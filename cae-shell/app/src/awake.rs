//! Keeping the machine awake, which is a Wayland protocol and not a setting.
//!
//! A compositor stops saying that nobody is there while a visible surface
//! asks it not to (`zwp_idle_inhibit_manager_v1`), and everything that acts
//! on idleness — the lock, the screen going dark — listens for it to say so.
//! GPUI does not speak that protocol, so this is a connection of its own, on
//! a thread of its own, the way the background is: one pixel of nothing in
//! the corner of the overlay, with an inhibitor on it.
//!
//! One pixel because it must be visible to count, and the overlay because a
//! surface under a fullscreen window is not.

use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::time::SystemTime;

use gpui::{App, Global};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::idle_inhibit::zv1::client::{zwp_idle_inhibit_manager_v1, zwp_idle_inhibitor_v1};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

const NAMESPACE: &str = "caelestia-awake";

/// Whether the machine is being kept awake, and since when, for the life of
/// the shell. A panel that asks for it is a window that will be shut again,
/// and the asking has to outlive it.
#[derive(Default)]
pub struct Keeper {
    /// Kept for its own sake: letting go of it is what lets the machine
    /// sleep again.
    held: Option<Awake>,
    since: Option<SystemTime>,
}

impl Global for Keeper {}

/// Since when the machine has been kept awake, or nothing if it is not.
pub fn kept(cx: &mut App) -> Option<SystemTime> {
    let keeper = cx.default_global::<Keeper>();
    keeper.held.as_ref().and(keeper.since)
}

/// Keeps the machine awake, or stops. Says whether it worked, which for
/// keeping is whether the compositor has the protocol at all.
pub fn keep(awake: bool, cx: &mut App) -> bool {
    if !awake {
        *cx.default_global::<Keeper>() = Keeper::default();
        return true;
    }
    match keep_awake() {
        Ok(held) => {
            *cx.default_global::<Keeper>() = Keeper { held: Some(held), since: Some(SystemTime::now()) };
            true
        }
        Err(error) => {
            eprintln!("cae: cannot keep the machine awake: {error}");
            false
        }
    }
}

/// The machine is kept awake for as long as this is held. Dropping it closes
/// the connection everything below was made on, which is how the compositor
/// hears that it may sleep again.
pub struct Awake {
    /// The thread watches this for its other end going, which is this being
    /// dropped.
    _wake: UnixStream,
}

/// Asks the compositor to stop noticing that nobody is there, until the
/// answer is dropped. An error when the compositor has no such protocol, or
/// there is no compositor.
fn keep_awake() -> Result<Awake, String> {
    let (wake, woken) = UnixStream::pair().map_err(|error| error.to_string())?;
    let (told, heard) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("awake".to_string())
        .spawn(move || {
            let held = hold(&woken);
            let started = held.as_ref().err().cloned();
            // Whoever asked is waiting to hear whether it worked, and then
            // never hears from this thread again.
            let _ = told.send(started);
            match held {
                Ok(watch) => watch(),
                Err(_) => {}
            }
        })
        .map_err(|error| error.to_string())?;
    match heard.recv().map_err(|error| error.to_string())? {
        Some(error) => Err(error),
        None => Ok(Awake { _wake: wake }),
    }
}

/// Everything the compositor needs to be told, and then a wait for the word
/// to stop. The wait is handed back rather than run, so that whoever asked
/// hears first whether any of it worked.
fn hold(woken: &UnixStream) -> Result<impl FnOnce() + use<>, String> {
    let connection = Connection::connect_to_env().map_err(|error| error.to_string())?;
    let (globals, mut events) = registry_queue_init::<Held>(&connection).map_err(|error| error.to_string())?;
    let queue = events.handle();
    let missing = |error: wayland_client::globals::BindError| format!("the compositor lacks something: {error}");

    let compositor: wl_compositor::WlCompositor = globals.bind(&queue, 4..=6, ()).map_err(missing)?;
    let shm: wl_shm::WlShm = globals.bind(&queue, 1..=2, ()).map_err(missing)?;
    let layers: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals.bind(&queue, 1..=4, ()).map_err(missing)?;
    let inhibitors: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 = globals.bind(&queue, 1..=1, ()).map_err(missing)?;

    let surface = compositor.create_surface(&queue, ());
    let layer = layers.get_layer_surface(&surface, None, zwlr_layer_shell_v1::Layer::Overlay, NAMESPACE.to_string(), &queue, ());
    layer.set_size(1, 1);
    layer.set_anchor(zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
    let nowhere = compositor.create_region(&queue, ());
    surface.set_input_region(Some(&nowhere));
    nowhere.destroy();
    surface.commit();

    // The size has to be agreed before anything may be shown.
    let mut held = Held::default();
    events.roundtrip(&mut held).map_err(|error| error.to_string())?;

    surface.attach(Some(&nothing(&shm, &queue)?), 0, 0);
    surface.damage(0, 0, 1, 1);
    surface.commit();
    inhibitors.create_inhibitor(&surface, &queue, ());
    events.roundtrip(&mut held).map_err(|error| error.to_string())?;

    // Held here, and dropped with everything else when the wait is over:
    // what the compositor was asked lasts exactly as long as the connection.
    let woken = woken.try_clone().map_err(|error| error.to_string())?;
    Ok(move || wait_until_dropped(connection, events, held, woken))
}

/// Answers the compositor until the other end of `woken` goes, which is the
/// `Awake` being dropped. Nothing else is listening: a layer surface is told
/// its size again whenever the screens change, and a configure nobody
/// acknowledges is one the compositor may act on by putting the surface away.
fn wait_until_dropped(connection: Connection, mut events: wayland_client::EventQueue<Held>, mut held: Held, woken: UnixStream) {
    loop {
        if events.dispatch_pending(&mut held).is_err() || connection.flush().is_err() {
            return;
        }
        let Some(reading) = events.prepare_read() else { continue };
        let listen = |fd: i32| libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let mut watching = [listen(reading.connection_fd().as_raw_fd()), listen(woken.as_raw_fd())];
        // SAFETY: two descriptors that are open for as long as the call.
        unsafe { libc::poll(watching.as_mut_ptr(), watching.len() as libc::nfds_t, -1) };
        let [compositor, dropped] = watching.map(|fd| fd.revents);
        if dropped != 0 || compositor & (libc::POLLERR | libc::POLLHUP) != 0 {
            return;
        }
        if compositor & libc::POLLIN != 0 && reading.read().is_err() {
            return;
        }
    }
}

/// One transparent pixel, which is what makes the surface a visible one.
fn nothing(shm: &wl_shm::WlShm, queue: &QueueHandle<Held>) -> Result<wl_buffer::WlBuffer, String> {
    // SAFETY: a name and no flags; the descriptor is owned from here on.
    let mut file = unsafe {
        let fd = libc::memfd_create(c"cae-awake".as_ptr(), 0);
        if fd < 0 {
            return Err("no memory to show a pixel from".to_string());
        }
        std::fs::File::from_raw_fd(fd)
    };
    file.write_all(&[0; 4]).map_err(|error| error.to_string())?;
    let pool = shm.create_pool(file.as_fd(), 4, queue, ());
    let buffer = pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, queue, ());
    pool.destroy();
    Ok(buffer)
}

/// Nothing is drawn and nothing is pressed, so there is no state to keep.
#[derive(Default)]
struct Held;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Held {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for Held {
    fn event(
        _: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, .. } = event {
            layer.ack_configure(serial);
        }
    }
}

delegate_noop!(Held: wl_compositor::WlCompositor);
delegate_noop!(Held: wayland_client::protocol::wl_region::WlRegion);
delegate_noop!(Held: wl_shm_pool::WlShmPool);
delegate_noop!(Held: ignore wl_buffer::WlBuffer);
delegate_noop!(Held: zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(Held: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1);
delegate_noop!(Held: zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1);
delegate_noop!(Held: ignore wl_shm::WlShm);
delegate_noop!(Held: ignore wl_surface::WlSurface);

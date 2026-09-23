//! The compositor's side of what the shell draws on its own: a Wayland
//! connection of its own, and layer surfaces on it, apart from GPUI.
//!
//! The background has one under everything on every screen; the cinema has
//! one over everything on the screen somebody is looking at. Both are drawn
//! on the card directly (`crate::card`), from a thread of their own, and
//! both are paced by the compositor: a frame is drawn when it has asked for
//! one, which it does at the screen's rate while the surface is shown and
//! not at all while the screen is off. `C` is what each surface is drawn on.

use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_callback, wl_compositor, wl_output, wl_region, wl_registry, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

pub use zwlr_layer_shell_v1::Layer;

use crate::card::{ARGB8888, XRGB8888};

/// The first version of `wl_output` that says what the output is called.
const NAMED_OUTPUTS: u32 = 4;

/// The modifier that says "laid out plainly".
const LINEAR: u64 = 0;

/// Where the surfaces go, and what they are.
pub struct Plan {
    pub layer: Layer,
    /// What a compositor rule knows them by.
    pub namespace: &'static str,
    /// The output to be on, by name; every output where there is none.
    pub on: Option<String>,
    /// Seen through wherever nothing is drawn, or covering the output whole.
    pub see_through: bool,
}

/// A surface on one screen.
pub struct Sheet<C> {
    /// Before the surface it draws on, which is the order they are dropped
    /// in: a swapchain must not outlive its surface.
    pub canvas: Option<C>,
    pub surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    viewport: wp_viewport::WpViewport,
    fraction: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    /// As the compositor last said, in its own units, and whether anything
    /// has been done about it yet.
    pub size: (u32, u32),
    pub resized: bool,
    /// How many of the screen's pixels one of those units is, in 120ths,
    /// where the compositor says so to a surface rather than of an output.
    scale: Option<u32>,
    /// Whether the compositor has been asked to say when it wants the next
    /// frame, and has not said yet.
    pub asked: bool,
    /// When a frame last went to the compositor.
    pub drawn: Option<Instant>,
}

impl<C> Sheet<C> {
    /// The size in the screen's own pixels, on an output that says it is
    /// `output_scale` to the unit where the surface has been told no better.
    pub fn pixels(&self, output_scale: i32) -> (u32, u32) {
        let scale = self.scale.unwrap_or(output_scale.max(1) as u32 * 120);
        // To the nearest, halves upwards, which is how the protocol has a
        // client round.
        let scaled = |length: u32| (length * scale + 60) / 120;
        (scaled(self.size.0), scaled(self.size.1))
    }

    /// Asks the compositor to say when it wants the next frame. Goes out
    /// with the next commit, whoever makes it.
    pub fn ask<D: Dispatch<wl_callback::WlCallback, u32> + 'static>(&mut self, queue: &QueueHandle<D>, global: u32) {
        self.surface.frame(queue, global);
        self.asked = true;
    }

    /// Has the compositor stretch whatever is drawn to the size it gave, and,
    /// for a surface that covers the output, tells it nothing under this
    /// needs drawing.
    pub fn fill<D: Dispatch<wl_region::WlRegion, ()> + 'static>(&self, compositor: &wl_compositor::WlCompositor, queue: &QueueHandle<D>, see_through: bool) {
        let (width, height) = (self.size.0 as i32, self.size.1 as i32);
        self.viewport.set_destination(width, height);
        if see_through {
            return;
        }
        let all = compositor.create_region(queue, ());
        all.add(0, 0, width, height);
        self.surface.set_opaque_region(Some(&all));
        all.destroy();
    }
}

impl<C> Drop for Sheet<C> {
    fn drop(&mut self) {
        self.canvas = None;
        if let Some(fraction) = &self.fraction {
            fraction.destroy();
        }
        self.viewport.destroy();
        self.layer.destroy();
        self.surface.destroy();
    }
}

pub struct Screen<C> {
    /// What the registry calls the output, which is how its going is said.
    pub global: u32,
    output: wl_output::WlOutput,
    /// What the compositor calls it: "eDP-1".
    pub name: String,
    /// Whole pixels to the unit, as an output says it.
    pub scale: i32,
    pub sheet: Option<Sheet<C>>,
}

impl<C> Drop for Screen<C> {
    fn drop(&mut self) {
        // The sheet first: it was made for this output.
        self.sheet = None;
        self.output.release();
    }
}

pub struct Desk<C> {
    pub compositor: wl_compositor::WlCompositor,
    layers: zwlr_layer_shell_v1::ZwlrLayerShellV1,
    viewporter: wp_viewporter::WpViewporter,
    /// Not every compositor has one, and one without it scales by whole
    /// numbers, which its outputs say.
    fractions: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    /// Where frames can be handed over as dma-bufs, and whether the plain
    /// kind the surfaces are drawn in is one the compositor has said it
    /// takes.
    pub dmabuf: Option<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
    pub takes_plain: bool,
    pub screens: Vec<Screen<C>>,
    pub plan: Plan,
}

/// The compositor's globals bound, and a screen for every output there is
/// already. One that comes later is an event.
pub fn open<C: 'static>(connection: &Connection, plan: Plan) -> Result<(EventQueue<Desk<C>>, Desk<C>), String> {
    let (globals, mut events) = registry_queue_init::<Desk<C>>(connection).map_err(|error| error.to_string())?;
    let queue = events.handle();
    let missing = |error: wayland_client::globals::BindError| format!("the compositor lacks something: {error}");
    let mut desk = Desk {
        compositor: globals.bind(&queue, 4..=6, ()).map_err(missing)?,
        layers: globals.bind(&queue, 1..=4, ()).map_err(missing)?,
        viewporter: globals.bind(&queue, 1..=1, ()).map_err(missing)?,
        fractions: globals.bind(&queue, 1..=1, ()).ok(),
        // Version three exactly: the one that lists what it takes as a
        // format and a modifier at a time, which is all this asks of it.
        dmabuf: globals.bind(&queue, 3..=3, ()).ok(),
        takes_plain: false,
        screens: Vec::new(),
        plan,
    };
    let outputs: Vec<(u32, u32)> = globals.contents().with_list(|all| {
        let is_output = |global: &&wayland_client::globals::Global| global.interface == wl_output::WlOutput::interface().name;
        all.iter().filter(is_output).map(|global| (global.name, global.version)).collect()
    });
    for (name, version) in outputs {
        desk.output_came(globals.registry(), name, version, &queue);
    }
    // What the compositor takes as a dma-buf is said as the global is bound,
    // and every output's name as it is: heard now, before the first surface
    // is given anything to draw on.
    events.roundtrip(&mut desk).map_err(|error| error.to_string())?;
    Ok((events, desk))
}

impl<C: 'static> Desk<C> {
    /// The surface on every screen that has one.
    pub fn sheets(&mut self) -> impl Iterator<Item = &mut Sheet<C>> {
        self.screens.iter_mut().filter_map(|screen| screen.sheet.as_mut())
    }

    /// Puts a surface on the first output there is, where the one the plan
    /// named has none: an output it could not find is no reason to show
    /// nothing at all.
    pub fn fall_back(&mut self, queue: &QueueHandle<Desk<C>>) {
        if self.screens.iter().any(|screen| screen.sheet.is_some()) {
            return;
        }
        let Some(first) = self.screens.first() else { return };
        let (output, global) = (first.output.clone(), first.global);
        let sheet = self.sheet(&output, global, queue);
        self.screens[0].sheet = Some(sheet);
    }

    fn wants(&self, name: &str) -> bool {
        self.plan.on.as_deref().is_none_or(|on| on == name)
    }

    fn output_came(&mut self, registry: &wl_registry::WlRegistry, global: u32, version: u32, queue: &QueueHandle<Desk<C>>) {
        if version < NAMED_OUTPUTS {
            return eprintln!("cae: an output that cannot say its name is left without a {}", self.plan.namespace);
        }
        let output = registry.bind(global, NAMED_OUTPUTS, queue, global);
        self.screens.push(Screen { global, output, name: String::new(), scale: 1, sheet: None });
    }

    fn sheet(&self, output: &wl_output::WlOutput, global: u32, queue: &QueueHandle<Desk<C>>) -> Sheet<C> {
        let surface = self.compositor.create_surface(queue, ());
        let layer = self.layers.get_layer_surface(&surface, Some(output), self.plan.layer, self.plan.namespace.to_string(), queue, global);
        layer.set_anchor(zwlr_layer_surface_v1::Anchor::all());
        // Under the bar's strip as well: nothing is reserved against it.
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        // Nothing here to press, so a press lands on whatever is under it,
        // as though it were not there.
        let nowhere = self.compositor.create_region(queue, ());
        surface.set_input_region(Some(&nowhere));
        nowhere.destroy();
        let viewport = self.viewporter.get_viewport(&surface, queue, ());
        let fraction = self.fractions.as_ref().map(|fractions| fractions.get_fractional_scale(&surface, queue, global));
        // The first commit has no picture: it asks what size to be.
        surface.commit();
        Sheet { canvas: None, surface, layer, viewport, fraction, size: (0, 0), resized: false, scale: None, asked: false, drawn: None }
    }
}

/// Sleeps until the compositor says something, somebody writes to `woken`,
/// or it is `until`.
pub fn wait<D>(connection: &Connection, events: &EventQueue<D>, mut woken: Option<&mut UnixStream>, until: Option<Instant>) -> Result<(), String> {
    connection.flush().map_err(|error| error.to_string())?;
    // Nothing to wait for when something has already arrived.
    let Some(reading) = events.prepare_read() else { return Ok(()) };

    let listen = |fd: i32| libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    // A descriptor below nought is one poll leaves alone.
    let waker_fd = woken.as_ref().map_or(-1, |woken| woken.as_raw_fd());
    let mut heard = [listen(reading.connection_fd().as_raw_fd()), listen(waker_fd)];
    // Rounded up: a wait that ends a fraction early is a loop that spins.
    let patience = until.map_or(-1, |until| until.saturating_duration_since(Instant::now()).as_millis() as i32 + 1);
    // SAFETY: two descriptors that are open for as long as the call.
    unsafe { libc::poll(heard.as_mut_ptr(), heard.len() as libc::nfds_t, patience) };

    let [compositor, waker] = heard.map(|fd| fd.revents);
    if compositor & (libc::POLLERR | libc::POLLHUP) != 0 {
        return Err("the compositor has gone".to_string());
    }
    if compositor & libc::POLLIN != 0 {
        reading.read().map_err(|error| error.to_string())?;
    }
    if let Some(woken) = woken.as_mut().filter(|_| waker != 0) {
        // Only that it was written to matters, not how often.
        let mut written = [0; 64];
        while woken.read(&mut written).is_ok_and(|read| read == written.len()) {}
    }
    Ok(())
}

impl<C: 'static> Dispatch<wl_registry::WlRegistry, GlobalListContents> for Desk<C> {
    fn event(
        desk: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global { name, interface, version } if interface == wl_output::WlOutput::interface().name => {
                desk.output_came(registry, name, version, queue);
            }
            wl_registry::Event::GlobalRemove { name } => desk.screens.retain(|screen| screen.global != name),
            _ => {}
        }
    }
}

impl<C: 'static> Dispatch<wl_output::WlOutput, u32> for Desk<C> {
    fn event(desk: &mut Self, output: &wl_output::WlOutput, event: wl_output::Event, global: &u32, _: &Connection, queue: &QueueHandle<Self>) {
        let Some(at) = desk.screens.iter().position(|screen| screen.global == *global) else { return };
        match event {
            wl_output::Event::Name { name } => desk.screens[at].name = name,
            wl_output::Event::Scale { factor } => desk.screens[at].scale = factor,
            // Everything about the output has been said, its name included.
            wl_output::Event::Done if desk.screens[at].sheet.is_none() && desk.wants(&desk.screens[at].name) => {
                desk.screens[at].sheet = Some(desk.sheet(output, *global, queue));
            }
            _ => {}
        }
    }
}

impl<C: 'static> Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, u32> for Desk<C> {
    fn event(
        desk: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(screen) = desk.screens.iter_mut().find(|screen| screen.global == *global) else { return };
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                layer.ack_configure(serial);
                // Said again whenever anything on the output reserves a strip
                // of it, at the same size as before: only a new size is
                // something to make a new picture for.
                if let Some(sheet) = screen.sheet.as_mut().filter(|sheet| sheet.size != (width, height)) {
                    (sheet.size, sheet.resized) = ((width, height), true);
                }
            }
            zwlr_layer_surface_v1::Event::Closed => screen.sheet = None,
            _ => {}
        }
    }
}

impl<C: 'static> Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, u32> for Desk<C> {
    fn event(
        desk: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = event else { return };
        let sheet = desk.screens.iter_mut().find(|screen| screen.global == *global).and_then(|screen| screen.sheet.as_mut());
        if let Some(sheet) = sheet.filter(|sheet| sheet.scale != Some(scale)) {
            (sheet.scale, sheet.resized) = (Some(scale), true);
        }
    }
}

impl<C: 'static> Dispatch<wl_callback::WlCallback, u32> for Desk<C> {
    fn event(desk: &mut Self, _: &wl_callback::WlCallback, event: wl_callback::Event, global: &u32, _: &Connection, _: &QueueHandle<Self>) {
        let wl_callback::Event::Done { .. } = event else { return };
        let sheet = desk.screens.iter_mut().find(|screen| screen.global == *global).and_then(|screen| screen.sheet.as_mut());
        if let Some(sheet) = sheet {
            sheet.asked = false;
        }
    }
}

impl<C: 'static> Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for Desk<C> {
    fn event(desk: &mut Self, _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, event: zwp_linux_dmabuf_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let drawn_in = if desk.plan.see_through { ARGB8888 } else { XRGB8888 };
        if let zwp_linux_dmabuf_v1::Event::Modifier { format, modifier_hi, modifier_lo } = event
            && format == drawn_in
            && (u64::from(modifier_hi) << 32 | u64::from(modifier_lo)) == LINEAR
        {
            desk.takes_plain = true;
        }
    }
}

impl<C: 'static> Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for Desk<C> {
    fn event(desk: &mut Self, _: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, event: zwp_linux_buffer_params_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zwp_linux_buffer_params_v1::Event::Failed = event {
            // Swapchains from here on: whatever the compositor said, it will
            // say it again.
            eprintln!("cae: the {}: the compositor would not take a frame directly, so it goes through a swapchain", desk.plan.namespace);
            desk.takes_plain = false;
        }
    }
}

/// Whether the compositor still holds a frame handed to it, which it says it
/// does not once it has let go.
impl<C: 'static> Dispatch<wl_buffer::WlBuffer, Arc<AtomicBool>> for Desk<C> {
    fn event(_: &mut Self, _: &wl_buffer::WlBuffer, event: wl_buffer::Event, held: &Arc<AtomicBool>, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = event {
            held.store(false, Ordering::Release);
        }
    }
}

delegate_noop!(@<C: 'static> Desk<C>: wl_compositor::WlCompositor);
delegate_noop!(@<C: 'static> Desk<C>: wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(@<C: 'static> Desk<C>: wl_region::WlRegion);
delegate_noop!(@<C: 'static> Desk<C>: zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(@<C: 'static> Desk<C>: wp_viewporter::WpViewporter);
delegate_noop!(@<C: 'static> Desk<C>: wp_viewport::WpViewport);
// Which outputs it is on, which is the one it was put on.
delegate_noop!(@<C: 'static> Desk<C>: ignore wl_surface::WlSurface);

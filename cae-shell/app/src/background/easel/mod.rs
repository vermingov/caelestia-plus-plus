//! What draws the desktop's background: a helix that turns, or a picture.
//!
//! GPUI draws rectangles, text and paths. The helix is a scene of a few
//! hundred lit shapes sixty times a second, and a picture the size of a
//! screen is one it would keep a second copy of in memory for as long as it
//! was shown. So the background has a Wayland connection and a surface of
//! its own, under everything, and is drawn there on Vulkan directly. On a
//! thread of its own: it shares nothing with the interface but what it is
//! told to show.
//!
//! It costs what it is seen to cost. A frame of the helix is drawn only
//! when the compositor has shown the last one and asked for another — which
//! it does at the screen's rate while the surface is on screen, and not at
//! all while the screen is off or the session is locked — and only for a
//! screen whose desktop is in view. A picture is drawn while it is taking
//! over from another and not again. The rest of the time, which is most of a
//! working day, the thread sleeps until somebody writes to it.

mod card;
mod desk;
mod direct;
mod helix;
mod paint;

use std::ffi::c_void;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_compositor, wl_output};
use wayland_client::{Connection, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1;

use desk::{Desk, Sheet};
pub use helix::Colours;
use helix::Helix;
use paint::{Painted, Painter};

/// The most one frame may move the helix on. A frame that was late must not
/// make it jump.
const LONGEST_STEP: f64 = 0.25;

/// How long one picture takes to take over from another.
const CHANGE: Duration = Duration::from_millis(700);

/// How long the background waits to start again after falling over, the
/// first time and at most. A card that was lost, as one can be across a
/// suspend, is there again a moment later; one that fails at once is not
/// asked again at the pace of a loop.
const AGAIN_FIRST: Duration = Duration::from_secs(1);
const AGAIN_LAST: Duration = Duration::from_secs(16);

/// How many times in a row it may fall over straight after starting before
/// it is left down: something that fails that often is not going to stop.
const QUICK_FALLS: u32 = 5;

/// How long a run has to have lasted for its fall to be one of its own
/// rather than the last one again.
const LASTED: Duration = Duration::from_secs(30);

/// The least time between two frames on one screen. A screen faster than
/// sixty frames a second is drawn on every other time it asks, or every
/// third: the helix is slow, and each frame past sixty is work nobody sees.
const QUICKEST: Duration = Duration::from_millis(12);

#[derive(Clone, Debug, PartialEq)]
pub enum Showing {
    Helix(Colours),
    Picture(PathBuf),
}

/// What it is told to be, by whoever started it.
#[derive(Clone, Debug, PartialEq)]
pub struct Wishes {
    pub showing: Showing,
    /// The outputs whose desktop can be seen, by name. The helix turns on
    /// those and holds still on the rest.
    pub seen: Vec<String>,
}

struct Told {
    /// Shared rather than copied: the thread reads them for every frame.
    wishes: Arc<Wishes>,
    stop: bool,
}

/// The background, for as long as this is kept.
pub struct Easel {
    told: Arc<Mutex<Told>>,
    wake: UnixStream,
}

impl Easel {
    pub fn start(wishes: Wishes) -> std::io::Result<Easel> {
        let (wake, woken) = UnixStream::pair()?;
        woken.set_nonblocking(true)?;
        let told = Arc::new(Mutex::new(Told { wishes: Arc::new(wishes), stop: false }));
        let heard = told.clone();
        std::thread::Builder::new().name("background".to_string()).spawn(move || keep_running(&heard, woken))?;
        Ok(Easel { told, wake })
    }

    pub fn wish(&self, wishes: Wishes) {
        self.tell(|told| told.wishes = Arc::new(wishes));
    }

    fn tell(&self, change: impl FnOnce(&mut Told)) {
        change(&mut self.told.lock().unwrap_or_else(PoisonError::into_inner));
        // A thread that has stopped by itself is not listening, and that is
        // already known.
        let _ = (&self.wake).write(&[0]);
    }
}

impl Drop for Easel {
    fn drop(&mut self) {
        self.tell(|told| told.stop = true);
    }
}

/// How far into its turning the helix is. Counted a frame at a time and not
/// read off a clock: a desktop that comes back into view must find it where
/// it was left, not where it would have got to.
#[derive(Default)]
struct Turning {
    elapsed: f64,
    last_frame: Option<Instant>,
}

impl Turning {
    fn on(&mut self, now: Instant) -> f64 {
        let step = self.last_frame.map_or(0., |last| now.duration_since(last).as_secs_f64().min(LONGEST_STEP));
        self.last_frame = Some(now);
        self.elapsed += step;
        self.elapsed
    }

    fn rest(&mut self) {
        self.last_frame = None;
    }
}

/// `picture` cut to fill `size` exactly: scaled until it covers, and what
/// hangs over the edges taken off evenly.
fn cut(picture: &image::DynamicImage, (width, height): (u32, u32)) -> image::RgbaImage {
    picture.resize_to_fill(width, height, image::imageops::FilterType::CatmullRom).into_rgba8()
}

/// The compositor's globals bound, and a screen for every output there is
/// already. One that comes later is an event.
fn open(connection: &Connection) -> Result<(EventQueue<Desk>, Desk), String> {
    let (globals, mut events) = registry_queue_init::<Desk>(connection).map_err(|error| error.to_string())?;
    let queue = events.handle();
    let missing = |error: wayland_client::globals::BindError| format!("the compositor lacks something: {error}");
    let mut desk = Desk::new(
        globals.bind(&queue, 4..=6, ()).map_err(missing)?,
        globals.bind(&queue, 1..=4, ()).map_err(missing)?,
        globals.bind(&queue, 1..=1, ()).map_err(missing)?,
        globals.bind(&queue, 1..=1, ()).ok(),
        // Version three exactly: the one that lists what it takes as a
        // format and a modifier at a time, which is all this asks of it.
        globals.bind(&queue, 3..=3, ()).ok(),
    );
    let outputs: Vec<(u32, u32)> = globals.contents().with_list(|all| {
        let is_output = |global: &&wayland_client::globals::Global| global.interface == wl_output::WlOutput::interface().name;
        all.iter().filter(is_output).map(|global| (global.name, global.version)).collect()
    });
    for (name, version) in outputs {
        desk.output_came(globals.registry(), name, version, &queue);
    }
    // What the compositor takes as a dma-buf is said as the global is bound:
    // heard now, before the first screen is given anything to draw on.
    events.roundtrip(&mut desk).map_err(|error| error.to_string())?;
    Ok((events, desk))
}

/// What the thread keeps between one frame and the next.
struct Work {
    display: NonNull<c_void>,
    queue: QueueHandle<Desk>,
    painter: Painter,
    helix: Helix,
    /// What was shown last time round, to know what is new this time.
    shown: Option<Showing>,
    turning: Turning,
    /// When one picture began taking over from another, until it has.
    changing_since: Option<Instant>,
    /// When to look again without being asked: the end of a picture taking
    /// over from another, which nothing else would wake the thread for.
    again: Option<Instant>,
}

impl Work {
    fn show(&mut self, desk: &mut Desk, wishes: &Wishes) -> Result<(), String> {
        let new = self.shown.as_ref() != Some(&wishes.showing);
        let another_kind = self.shown.as_ref().map(std::mem::discriminant) != Some(std::mem::discriminant(&wishes.showing));
        if another_kind {
            // A picture was cut for the screen, and is not the helix's.
            desk.screens.iter_mut().filter_map(|screen| screen.sheet.as_mut()).for_each(|sheet| sheet.resized = true);
        }
        self.shown = Some(wishes.showing.clone());
        self.again = None;
        match &wishes.showing {
            Showing::Helix(colours) => self.show_helix(desk, wishes, colours, new),
            Showing::Picture(path) => self.show_picture(desk, path),
        }
    }

    /// A frame of the turning where the desktop can be seen and the
    /// compositor is ready for one, and a still wherever there is something
    /// new to show: a new size, or new colours.
    fn show_helix(&mut self, desk: &mut Desk, wishes: &Wishes, colours: &Colours, recoloured: bool) -> Result<(), String> {
        let now = Instant::now();
        let handing = handing_over(desk);
        let moving = desk.screens.iter().any(|screen| screen.sheet.is_some() && wishes.seen.contains(&screen.name));
        let time = if moving { self.turning.on(now) } else { self.turning.elapsed };
        if !moving {
            self.turning.rest();
        }

        for screen in &mut desk.screens {
            let Some(sheet) = screen.sheet.as_mut().filter(|sheet| sheet.size != (0, 0)) else { continue };
            let resized = std::mem::take(&mut sheet.resized);
            if resized {
                // Every pixel the screen has: the middle of the helix is in
                // focus, and sharp is what makes the rest read as blurred.
                let pixels = sheet.pixels(screen.scale);
                self.fit(sheet, pixels, &desk.compositor, handing.as_ref())?;
            }
            let turning = moving && wishes.seen.contains(&screen.name);
            let wanted = resized || recoloured || (turning && !sheet.asked);
            if !wanted {
                continue;
            }
            if !resized && !recoloured && sheet.drawn.is_some_and(|drawn| now.duration_since(drawn) < QUICKEST) {
                // A screen faster than the helix needs: skip this refresh,
                // and ask to be told of the next.
                sheet.ask(&self.queue, screen.global);
                sheet.surface.commit();
                continue;
            }
            self.paint_helix(sheet, screen.global, time, colours, turning);
        }
        Ok(())
    }

    fn paint_helix(&mut self, sheet: &mut Sheet, global: u32, time: f64, colours: &Colours, turning: bool) {
        let Some(canvas) = sheet.canvas.as_mut() else { return };
        let size = canvas.size();
        let (shapes, axis) = self.helix.frame(size, time, colours);
        let with_alpha = |[r, g, b]: [f32; 3]| [r, g, b, 1.];
        let scene = card::Scene {
            size: [size.0 as f32, size.1 as f32],
            axis_origin: axis.origin,
            axis_direction: axis.direction,
            linearise: if self.painter.encodes() { 1. } else { 0. },
            spare: 0.,
            deep: with_alpha(colours.deep),
            primary: with_alpha(colours.primary),
            hot: with_alpha(colours.hot),
        };
        let (surface, queue) = (&sheet.surface, &self.queue);
        let asked = std::cell::Cell::new(false);
        let painted = self.painter.paint_helix(canvas, &scene, shapes, || {
            // Only while it turns: a still needs nothing after it.
            if turning {
                surface.frame(queue, global);
                asked.set(true);
            }
        });
        keep_track(sheet, painted, asked.get());
    }

    /// The picture on every screen, cut for each, taking over from whatever
    /// was there before it. Drawn while that lasts, and then left alone.
    fn show_picture(&mut self, desk: &mut Desk, path: &Path) -> Result<(), String> {
        let handing = handing_over(desk);
        let mut redraw = false;
        for screen in &mut desk.screens {
            let Some(sheet) = screen.sheet.as_mut().filter(|sheet| sheet.size != (0, 0)) else { continue };
            if std::mem::take(&mut sheet.resized) {
                // Every pixel the screen has: a picture is sharp.
                let pixels = sheet.pixels(screen.scale);
                self.fit(sheet, pixels, &desk.compositor, handing.as_ref())?;
                redraw = true;
            }
        }
        let hung = self.hang(desk, path);
        // Read after the hanging, which for a large photograph is most of a
        // second: the change is timed from when there is something to show.
        let now = Instant::now();
        if hung {
            self.changing_since = Some(now);
        }

        let done = self.changing_since.map_or(1., |since| now.duration_since(since).as_secs_f32() / CHANGE.as_secs_f32());
        if done >= 1. && self.changing_since.take().is_some() {
            for sheet in desk.sheets() {
                if let Some(canvas) = sheet.canvas.as_mut() {
                    self.painter.settle(canvas);
                }
            }
            redraw = true;
        }
        let changing = self.changing_since.is_some();
        if let Some(since) = self.changing_since {
            // Woken for the end of it whether or not the compositor asks for
            // frames in between: a screen that is not shown asks for none.
            self.again = Some(since + CHANGE);
        }
        if !redraw && !changing {
            return Ok(());
        }
        // Quick to leave and slow to arrive, as everything the shell fades is.
        let eased = 1. - (1. - done.clamp(0., 1.)).powi(3);
        for screen in &mut desk.screens {
            let Some(sheet) = screen.sheet.as_mut() else { continue };
            if changing && sheet.asked && !redraw {
                continue;
            }
            let Some(canvas) = sheet.canvas.as_mut() else { continue };
            let (surface, queue, global) = (&sheet.surface, &self.queue, screen.global);
            let asked = std::cell::Cell::new(false);
            let painted = self.painter.paint_picture(canvas, eased, || {
                if changing {
                    surface.frame(queue, global);
                    asked.set(true);
                }
            });
            keep_track(sheet, painted, asked.get());
        }
        Ok(())
    }

    /// Hangs the picture at `path` on every screen that does not have it,
    /// and says whether that was any of them.
    fn hang(&mut self, desk: &mut Desk, path: &Path) -> bool {
        let mut without: Vec<&mut Sheet> =
            desk.sheets().filter(|sheet| sheet.canvas.is_some() && sheet.hung.as_deref() != Some(path)).collect();
        if without.is_empty() {
            return false;
        }
        // Whatever comes of it: a picture that cannot be opened is not
        // opened again with every frame.
        without.iter_mut().for_each(|sheet| sheet.hung = Some(path.to_path_buf()));
        // Opened for as long as it takes to cut, and not kept: a photograph
        // opened up is a hundred megabytes.
        let picture = match image::open(path) {
            Ok(picture) => picture,
            Err(error) => {
                eprintln!("cae: the background: {}: {error}", path.display());
                return false;
            }
        };
        for sheet in without {
            let canvas = sheet.canvas.as_mut().expect("kept for having one");
            let pixels = cut(&picture, canvas.size());
            self.painter.hang(canvas, &pixels);
        }
        true
    }

    /// Gives `sheet` something of `pixels` to draw on, its first if it has
    /// none, and has the compositor stretch it over the screen. Handed to the
    /// compositor directly where `dmabuf` is there to take it.
    fn fit(
        &mut self,
        sheet: &mut Sheet,
        pixels: (u32, u32),
        compositor: &wl_compositor::WlCompositor,
        dmabuf: Option<&zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
    ) -> Result<(), String> {
        if sheet.canvas.is_none() {
            let surface = NonNull::new(sheet.surface.id().as_ptr().cast::<c_void>()).ok_or("a surface with no address")?;
            // SAFETY: the display is the thread's own connection, which
            // outlives everything made on it, and a sheet drops its canvas
            // before it destroys its surface.
            sheet.canvas = Some(unsafe { self.painter.canvas(self.display, surface) }?);
        }
        sheet.fill(compositor, &self.queue);
        // A picture on it was cut for the size it had.
        sheet.hung = None;
        // A frame asked for before is answered all the same; until then the
        // new one is drawn on as soon as it can be.
        sheet.asked = false;
        let wayland = dmabuf.map(|dmabuf| direct::Wayland { dmabuf, queue: &self.queue, surface: &sheet.surface });
        self.painter.size(sheet.canvas.as_mut().expect("just made"), pixels, wayland)
    }
}

/// Where frames can be handed to the compositor directly: where it has said
/// it takes the plain kind, unless asked to use a swapchain regardless —
/// `CAE_BACKGROUND_SWAPCHAIN=1`, for a driver that says it can and cannot.
fn handing_over(desk: &Desk) -> Option<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1> {
    let asked_not_to = std::env::var_os("CAE_BACKGROUND_SWAPCHAIN").is_some();
    desk.dmabuf.clone().filter(|_| desk.takes_plain && !asked_not_to)
}

/// Keeps track of a frame that was drawn, or has to be tried again.
fn keep_track(sheet: &mut Sheet, painted: Painted, asked: bool) {
    match painted {
        Painted::Shown => {
            sheet.drawn = Some(Instant::now());
            sheet.asked |= asked;
        }
        // No image was free: the compositor lets go of one by saying so on
        // the connection this thread sleeps on, which wakes it to try again —
        // no timer needed.
        Painted::NotNow => {
            // A frame asked for and never sent would leave the request waiting
            // on a commit that never comes: send it bare.
            if asked {
                sheet.surface.commit();
                sheet.asked = true;
            }
        }
    }
}

/// Runs the background until it is told to stop, and starts it again when
/// it falls over: a card lost across a suspend used to leave the desktop
/// bare until the shell itself was restarted.
fn keep_running(told: &Mutex<Told>, mut woken: UnixStream) {
    let (mut again, mut quick_falls) = (AGAIN_FIRST, 0);
    loop {
        let started = Instant::now();
        let Err(error) = run(told, &mut woken) else { return };
        eprintln!("cae: the background has stopped: {error}");
        if started.elapsed() >= LASTED {
            (again, quick_falls) = (AGAIN_FIRST, 0);
        }
        quick_falls += 1;
        if quick_falls > QUICK_FALLS {
            return eprintln!("cae: the background keeps falling over, and is left down");
        }
        if rest(told, &mut woken, again) {
            return;
        }
        again = (again * 2).min(AGAIN_LAST);
    }
}

/// Waits for `how_long`, or until told to stop, and says which.
fn rest(told: &Mutex<Told>, woken: &mut UnixStream, how_long: Duration) -> bool {
    let until = Instant::now() + how_long;
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        cae_core::children::readable(woken, Some(left));
        let mut written = [0; 64];
        while woken.read(&mut written).is_ok_and(|read| read > 0) {}
        if told.lock().unwrap_or_else(PoisonError::into_inner).stop {
            return true;
        }
    }
    false
}

fn run(told: &Mutex<Told>, woken: &mut UnixStream) -> Result<(), String> {
    let connection = Connection::connect_to_env().map_err(|error| error.to_string())?;
    let (mut events, mut desk) = open(&connection)?;
    let mut work = Work {
        display: NonNull::new(connection.backend().display_ptr().cast::<c_void>()).ok_or("the connection has no display")?,
        queue: events.handle(),
        painter: Painter::new()?,
        helix: Helix::default(),
        shown: None,
        turning: Turning::default(),
        changing_since: None,
        again: None,
    };

    loop {
        events.dispatch_pending(&mut desk).map_err(|error| error.to_string())?;
        if work.painter.refused() {
            return Err("the graphics card refused it".to_string());
        }
        let wishes = {
            let told = told.lock().unwrap_or_else(PoisonError::into_inner);
            if told.stop {
                return Ok(());
            }
            told.wishes.clone()
        };
        work.show(&mut desk, &wishes)?;
        wait(&connection, &events, woken, work.again)?;
    }
}

/// Sleeps until the compositor says something, somebody writes to `woken`,
/// or it is `until`.
fn wait(connection: &Connection, events: &EventQueue<Desk>, woken: &mut UnixStream, until: Option<Instant>) -> Result<(), String> {
    connection.flush().map_err(|error| error.to_string())?;
    // Nothing to wait for when something has already arrived.
    let Some(reading) = events.prepare_read() else { return Ok(()) };

    let listen = |fd: i32| libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    let mut heard = [listen(reading.connection_fd().as_raw_fd()), listen(woken.as_raw_fd())];
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
    if waker != 0 {
        // Only that it was written to matters, not how often.
        let mut written = [0; 64];
        while woken.read(&mut written).is_ok_and(|read| read == written.len()) {}
    }
    Ok(())
}

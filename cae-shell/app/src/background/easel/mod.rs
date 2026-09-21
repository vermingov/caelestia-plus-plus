//! What draws the desktop's background: a helix that turns, or a picture.
//!
//! GPUI draws rectangles, text and paths. The helix is a fragment shader,
//! which it has no way to run, and a picture the size of a screen is one it
//! would keep a second copy of in memory for as long as it was shown. So the
//! background has a Wayland connection and a surface of its own, under
//! everything, and is drawn there with the same wgpu the rest of the shell is
//! drawn with. On a thread of its own: it shares nothing with the interface
//! but what it is told to show.
//!
//! It costs what it is seen to cost. The helix is drawn only for a screen
//! whose desktop is in view, at half the screen's size, thirty times a
//! second; a picture is drawn while it is taking over from another and not
//! again. The rest of the time, which is most of a working day, the thread
//! sleeps until somebody writes to it.

mod desk;
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

use desk::{Desk, Sheet};
pub use paint::Colours;
use paint::Painter;

/// Where the helix's clock starts again. A whole number of turns of both
/// strands, so the join cannot be seen, and soon enough that a float still
/// counts the phase exactly: 2π · 50 / 0.35.
const WRAP: f32 = 897.597_9;

/// The most one frame may move the helix on. A frame that was late must not
/// make it jump.
const LONGEST_STEP: f32 = 0.25;

/// How long one picture takes to take over from another, and how often that
/// is drawn while it does.
const CHANGE: Duration = Duration::from_millis(700);
const CHANGE_FRAME: Duration = Duration::from_millis(16);

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
    /// How long each frame of the helix is shown for.
    pub frame: Duration,
}

struct Told {
    wishes: Wishes,
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
        let told = Arc::new(Mutex::new(Told { wishes, stop: false }));
        let heard = told.clone();
        std::thread::Builder::new().name("background".to_string()).spawn(move || {
            if let Err(error) = run(&heard, woken) {
                eprintln!("cae: the background has stopped: {error}");
            }
        })?;
        Ok(Easel { told, wake })
    }

    pub fn wish(&self, wishes: Wishes) {
        self.tell(|told| told.wishes = wishes);
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
    elapsed: f32,
    last_frame: Option<Instant>,
}

impl Turning {
    fn on(&mut self, now: Instant) -> f32 {
        let step = self.last_frame.map_or(0., |last| now.duration_since(last).as_secs_f32().min(LONGEST_STEP));
        self.last_frame = Some(now);
        self.elapsed = (self.elapsed + step) % WRAP;
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
    let (globals, events) = registry_queue_init::<Desk>(connection).map_err(|error| error.to_string())?;
    let queue = events.handle();
    let missing = |error: wayland_client::globals::BindError| format!("the compositor lacks something: {error}");
    let mut desk = Desk::new(
        globals.bind(&queue, 4..=6, ()).map_err(missing)?,
        globals.bind(&queue, 1..=4, ()).map_err(missing)?,
        globals.bind(&queue, 1..=1, ()).map_err(missing)?,
        globals.bind(&queue, 1..=1, ()).ok(),
    );
    let outputs: Vec<(u32, u32)> = globals.contents().with_list(|all| {
        let is_output = |global: &&wayland_client::globals::Global| global.interface == wl_output::WlOutput::interface().name;
        all.iter().filter(is_output).map(|global| (global.name, global.version)).collect()
    });
    for (name, version) in outputs {
        desk.output_came(globals.registry(), name, version, &queue);
    }
    Ok((events, desk))
}

/// What the thread keeps between one frame and the next.
struct Work {
    display: NonNull<c_void>,
    queue: QueueHandle<Desk>,
    painter: Painter,
    /// What was shown last time round, to know what is new this time.
    shown: Option<Showing>,
    turning: Turning,
    /// When one picture began taking over from another, until it has.
    changing_since: Option<Instant>,
    next_frame: Option<Instant>,
}

impl Work {
    fn show(&mut self, desk: &mut Desk, wishes: &Wishes) -> Result<(), String> {
        let new = self.shown.as_ref() != Some(&wishes.showing);
        let another_kind = self.shown.as_ref().map(std::mem::discriminant) != Some(std::mem::discriminant(&wishes.showing));
        if another_kind {
            // The two are drawn at different sizes.
            desk.screens.iter_mut().filter_map(|screen| screen.sheet.as_mut()).for_each(|sheet| sheet.resized = true);
        }
        self.shown = Some(wishes.showing.clone());
        match &wishes.showing {
            Showing::Helix(colours) => self.show_helix(desk, wishes, colours, new),
            Showing::Picture(path) => self.show_picture(desk, path),
        }
    }

    /// A frame of the turning where the desktop can be seen, and a still
    /// wherever there is something new to show: a new size, or new colours.
    fn show_helix(&mut self, desk: &mut Desk, wishes: &Wishes, colours: &Colours, recoloured: bool) -> Result<(), String> {
        let now = Instant::now();
        let moving = desk.screens.iter().any(|screen| screen.sheet.is_some() && wishes.seen.contains(&screen.name));
        let frame_due = moving && self.next_frame.is_none_or(|due| now >= due);
        let time = if frame_due { self.turning.on(now) } else { self.turning.elapsed };
        if !moving {
            self.turning.rest();
        }
        self.next_frame = match (moving, frame_due) {
            (false, _) => None,
            (true, true) => Some(now + wishes.frame),
            (true, false) => self.next_frame,
        };

        for screen in &mut desk.screens {
            let Some(sheet) = screen.sheet.as_mut().filter(|sheet| sheet.size != (0, 0)) else { continue };
            let resized = std::mem::take(&mut sheet.resized);
            if resized {
                // Half the size, and the compositor stretches it: nothing in
                // it is sharp enough for that to show, and it is a quarter
                // of the work.
                let half = (sheet.size.0.div_ceil(2), sheet.size.1.div_ceil(2));
                self.fit(sheet, half, &desk.compositor)?;
            }
            let Some(canvas) = &sheet.canvas else { continue };
            if resized || recoloured || (frame_due && wishes.seen.contains(&screen.name)) {
                self.painter.paint_helix(canvas, time, colours);
            }
        }
        Ok(())
    }

    /// The picture on every screen, cut for each, taking over from whatever
    /// was there before it. Drawn while that lasts, and then left alone.
    fn show_picture(&mut self, desk: &mut Desk, path: &Path) -> Result<(), String> {
        let mut redraw = false;
        for screen in &mut desk.screens {
            let Some(sheet) = screen.sheet.as_mut().filter(|sheet| sheet.size != (0, 0)) else { continue };
            if std::mem::take(&mut sheet.resized) {
                // Every pixel the screen has: a picture is sharp.
                let pixels = sheet.pixels(screen.scale);
                self.fit(sheet, pixels, &desk.compositor)?;
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
            desk.canvases().for_each(|canvas| self.painter.settle(canvas));
            redraw = true;
        }
        if redraw || self.changing_since.is_some() {
            // Quick to leave and slow to arrive, as everything the shell
            // fades is.
            let eased = 1. - (1. - done.clamp(0., 1.)).powi(3);
            desk.canvases().for_each(|canvas| self.painter.paint_picture(canvas, eased));
        }
        self.next_frame = self.changing_since.map(|_| now + CHANGE_FRAME);
        Ok(())
    }

    /// Hangs the picture at `path` on every screen that does not have it,
    /// and says whether that was any of them.
    fn hang(&mut self, desk: &mut Desk, path: &Path) -> bool {
        let mut without: Vec<&mut Sheet> = desk
            .screens
            .iter_mut()
            .filter_map(|screen| screen.sheet.as_mut())
            .filter(|sheet| sheet.canvas.is_some() && sheet.hung.as_deref() != Some(path))
            .collect();
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
            self.painter.hang(canvas, &cut(&picture, canvas.size()));
        }
        true
    }

    /// Gives `sheet` a swapchain of `pixels`, its first if it has none, and
    /// has the compositor stretch it over the screen.
    fn fit(&mut self, sheet: &mut Sheet, pixels: (u32, u32), compositor: &wl_compositor::WlCompositor) -> Result<(), String> {
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
        self.painter.size(sheet.canvas.as_mut().expect("just made"), pixels.0, pixels.1)
    }
}

fn run(told: &Mutex<Told>, mut woken: UnixStream) -> Result<(), String> {
    let connection = Connection::connect_to_env().map_err(|error| error.to_string())?;
    let (mut events, mut desk) = open(&connection)?;
    let mut work = Work {
        display: NonNull::new(connection.backend().display_ptr().cast::<c_void>()).ok_or("the connection has no display")?,
        queue: events.handle(),
        painter: Painter::new(),
        shown: None,
        turning: Turning::default(),
        changing_since: None,
        next_frame: None,
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
        wait(&connection, &events, &mut woken, work.next_frame)?;
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

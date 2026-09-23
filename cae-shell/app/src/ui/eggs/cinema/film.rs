//! The cinema's own thread: a surface over everything on one screen, drawn
//! on the card whenever the compositor asks for a frame, until the track is
//! over — and then nothing, not even the thread.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use wayland_client::{Connection, Proxy};

use super::kit::{Film, Kit};
use super::marks::Marks;
use super::{OVER, Stage, acts, portrait};
use crate::card::{self, Card, Painted, Vulkan};
use crate::desk::{self, Layer, Plan};

/// Its own name: it is not a panel, and must not be blurred or animated
/// like one.
const NAMESPACE: &str = "caelestia-israelegg";

/// How tall the portrait is painted, as a share of the screen: as tall as it
/// is shown, and as much again as the camera pushes in on it.
const PORTRAIT: f32 = 0.52 * 1.42;

type Desk = desk::Desk<card::Canvas<()>>;

/// Whether it is playing now: once at a time.
static PLAYING: AtomicBool = AtomicBool::new(false);

/// Plays it on the output somebody is looking at, as Hyprland says, or on
/// the first there is where nothing says. Does nothing while it is already
/// playing.
pub fn play() {
    if PLAYING.swap(true, Ordering::AcqRel) {
        return;
    }
    let started = std::thread::Builder::new().name("cinema".to_string()).spawn(move || {
        if let Err(error) = run(cae_core::hypr::focused_monitor()) {
            eprintln!("cae: the cinema: {error}");
        }
        PLAYING.store(false, Ordering::Release);
    });
    if let Err(error) = started {
        eprintln!("cae: the cinema could not start: {error}");
        PLAYING.store(false, Ordering::Release);
    }
}

/// The track, at the volume it was always played at. Thrown at whatever is
/// installed: the picture runs whether or not anything answers.
fn play_the_track() {
    let track = cae_core::about::checkout().join("assets/israel.mp3");
    std::thread::spawn(move || {
        let players: [(&str, &[&str]); 2] =
            [("mpv", &["--no-video", "--really-quiet", "--volume=80"]), ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet", "-volume", "80"])];
        for (player, args) in players {
            if std::process::Command::new(player).args(args).arg(&track).status().is_ok() {
                return;
            }
        }
    });
}

fn run(on: Option<String>) -> Result<(), String> {
    let connection = Connection::connect_to_env().map_err(|error| error.to_string())?;
    // A name nothing has, where none was given: the first output, then.
    let plan = Plan { layer: Layer::Overlay, namespace: NAMESPACE, on: Some(on.unwrap_or_default()), see_through: true };
    let (mut events, mut desk) = desk::open::<card::Canvas<()>>(&connection, plan)?;
    let queue = events.handle();
    desk.fall_back(&queue);

    let display = NonNull::new(connection.backend().display_ptr().cast::<c_void>()).ok_or("the connection has no display")?;
    let vulkan = Vulkan::new()?;
    let mut drawing: Option<(Rc<Card>, Kit)> = None;
    let mut marks = Marks::with_room();
    // When the first frame went: the picture, and the track, start with it.
    let mut started: Option<Instant> = None;

    loop {
        events.dispatch_pending(&mut desk).map_err(|error| error.to_string())?;
        let now = started.map_or(0, |at| at.elapsed().as_millis());
        if now >= OVER {
            return Ok(());
        }
        let dmabuf = handing_over(&desk);
        let Some(screen) = desk.screens.iter_mut().find(|screen| screen.sheet.is_some()) else {
            // Its output went, or the compositor closed it.
            return if started.is_some() { Ok(()) } else { Err("there is no screen to play it on".to_string()) };
        };
        let (global, output_scale) = (screen.global, screen.scale);
        let sheet = screen.sheet.as_mut().expect("found for having one");

        if sheet.size != (0, 0) && std::mem::take(&mut sheet.resized) {
            if sheet.canvas.is_none() {
                let surface = NonNull::new(sheet.surface.id().as_ptr().cast::<c_void>()).ok_or("a surface with no address")?;
                // SAFETY: the display is the thread's own connection, which
                // outlives everything made on it, and a sheet drops its
                // canvas before it destroys its surface.
                sheet.canvas = Some(unsafe { card::Canvas::new(&vulkan, display, surface) }?);
            }
            sheet.fill(&desk.compositor, &queue, true);
            sheet.asked = false;
            let pixels = sheet.pixels(output_scale);
            let canvas = sheet.canvas.as_mut().expect("just made");
            if drawing.is_none() {
                let card = Card::for_surface(&vulkan, canvas.surface)?;
                let picture = portrait::paint((pixels.1 as f32 * PORTRAIT) as u32).ok_or("the portrait could not be painted")?;
                drawing = Some((card.clone(), Kit::new(&card, &picture)?));
            }
            let (card, kit) = drawing.as_ref().expect("just made");
            let wayland = dmabuf.as_ref().map(|dmabuf| card::Wayland { dmabuf, queue: &queue, surface: &sheet.surface });
            canvas.fit(card, pixels, wayland, &kit.spec).map_err(|fault| fault.said)?;
        }

        if let (Some((card, kit)), false) = (&drawing, sheet.asked)
            && let Some(canvas) = sheet.canvas.as_mut()
        {
            let (width, height) = canvas.size();
            let unit = width as f32 / sheet.size.0.max(1) as f32;
            let stage = Stage::at(now, (width as f32, height as f32), unit);
            marks.list.clear();
            acts::lay(&stage, &mut marks);
            let film = Film { size: [width as f32, height as f32], time: now as f32 / 1000., linearise: if card.encodes { 1. } else { 0. } };
            let count = marks.list.len() as u32;
            let surface = &sheet.surface;
            let asked = Cell::new(false);
            let painted = canvas.paint(
                || {
                    surface.frame(&queue, global);
                    asked.set(true);
                },
                None,
                |data| data.write(0, bytemuck::cast_slice(&marks.list)),
                |card, frame| kit.record(card, frame, &film, count),
            );
            match painted {
                Ok(Painted::Shown) => {
                    sheet.asked |= asked.get();
                    if started.is_none() {
                        play_the_track();
                        started = Some(Instant::now());
                    }
                }
                // No image was free: the compositor says when it lets one
                // go, on the connection this sleeps on. A frame asked for
                // and never sent would wait on a commit that never comes, so
                // the request goes bare.
                Ok(Painted::NotNow) => {
                    if asked.get() {
                        surface.commit();
                        sheet.asked = true;
                    }
                }
                Err(fault) => return Err(fault.said),
            }
        }

        // Woken by the compositor, or at the end whether or not it asks for
        // anything more — a screen that is off asks for no frames.
        let until = started.map(|at| at + Duration::from_millis(OVER as u64));
        desk::wait(&connection, &events, None, until)?;
    }
}

/// Where frames can be handed to the compositor directly: where it has said
/// it takes the plain kind, unless asked to use a swapchain regardless — by
/// the same switch as the background.
fn handing_over(desk: &Desk) -> Option<wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1> {
    let asked_not_to = std::env::var_os("CAE_BACKGROUND_SWAPCHAIN").is_some();
    desk.dmabuf.clone().filter(|_| desk.takes_plain && !asked_not_to)
}

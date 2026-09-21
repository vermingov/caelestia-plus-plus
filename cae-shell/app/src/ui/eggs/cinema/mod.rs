//! The other egg: five acts over a track, fullscreen, and nobody asked for
//! any of it.
//!
//! The QML one was two thousand lines of `Rectangle`s, two `Shape`s and a
//! `MediaPlayer`. This is the same picture from the same numbers, drawn with
//! paths instead, and driven by one clock: every act, every wobble and every
//! shake is read off the time since the thing started, so there is no state
//! to get out of step with the music.

mod acts;

use std::time::{Duration, Instant};

use gpui::{
    App, AppContext, Bounds, Context, Global, IntoElement, Pixels, Render, Size, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, canvas, div, layer_shell::*, point, prelude::*,
    px,
};

use super::paint::{self, Cam, shade};
use crate::ui::rsx;
use crate::ui::screen;

/// Its own name: it is not a panel and must not be blurred like one.
const NAMESPACE: &str = "caelestia-israelegg";

const FRAME: Duration = Duration::from_millis(16);

/// When each act takes over, in milliseconds from the start. The first
/// begins at nought, with the surface itself.
pub const FLAG: u128 = 4200;
pub const TANK: u128 = 7800;
pub const FACE: u128 = 10600;
pub const WAKE: u128 = 13200;

/// How long the track is. The last act runs until it ends rather than to a
/// clock of its own.
const TRACK: u128 = 17_984;
/// What the fade out takes, and how long after it the surface goes.
const FADE: u128 = 1200;
const OVER: u128 = TRACK + 1300;

/// Everything but the flash, the letterbox and the vignettes rides this.
const PUSH_IN: u128 = 26_000;

#[derive(Default)]
struct Open(Option<WindowHandle<Cinema>>);

impl Global for Open {}

const PIECE: &str = "egg";

/// Plays it, or asks the old shell to while the old shell still draws it.
pub fn pop(cx: &mut App) {
    crate::ours::when_known(PIECE, cx, |ours, cx| {
        if !ours {
            return cx.background_spawn(async { drop(cae_core::services::ipc("israelEgg", "pop", &[])) }).detach();
        }
        play(cx);
    });
}

fn play(cx: &mut App) {
    if cx.default_global::<Open>().0.is_some_and(|open| open.update(cx, |_, _, _| ()).is_ok()) {
        return;
    }
    let display = screen::focused_display(cx).or_else(|| screen::outputs(cx).first().map(|(_, display)| *display));
    let options = gpui::WindowOptions {
        titlebar: None,
        focus: false,
        display_id: display,
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            Size::new(screen::STRETCH, screen::STRETCH),
        ))),
        app_id: Some(NAMESPACE.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: NAMESPACE.to_string(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: Some(px(-1.)),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..crate::ui::surface::options()
    };
    match cx.open_window(options, |window, cx| cx.new(|cx| Cinema::new(window, cx))) {
        Ok(window) => cx.set_global(Open(Some(window))),
        Err(error) => eprintln!("cae: cannot play the egg: {error}"),
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
            let started = std::process::Command::new(player).args(args).arg(&track).status();
            if started.is_ok() {
                return;
            }
        }
    });
}

struct Cinema {
    from: Instant,
}

impl Cinema {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Cinema {
        play_the_track();
        cx.spawn(async move |cinema, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                let going = cinema.update(cx, |cinema: &mut Cinema, cx| {
                    if cinema.at() >= OVER {
                        cx.defer(away);
                        return false;
                    }
                    cx.notify();
                    true
                });
                if !matches!(going, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
        Cinema { from: Instant::now() }
    }

    fn at(&self) -> u128 {
        self.from.elapsed().as_millis()
    }
}

fn away(cx: &mut App) {
    let Some(open) = cx.default_global::<Open>().0.take() else { return };
    let _ = open.update(cx, |_, window, _| window.remove_window());
}

// -- The curves ------------------------------------------------------------

pub fn through(now: u128, from: u128, over: u128) -> f32 {
    if now <= from || over == 0 {
        return 0.;
    }
    (((now - from) as f32) / over as f32).min(1.)
}

pub fn in_quad(at: f32) -> f32 {
    at * at
}

pub fn out_quad(at: f32) -> f32 {
    at * (2. - at)
}

pub fn in_out_quad(at: f32) -> f32 {
    if at < 0.5 { 2. * at * at } else { -1. + (4. - 2. * at) * at }
}

pub fn in_cubic(at: f32) -> f32 {
    at * at * at
}

pub fn out_cubic(at: f32) -> f32 {
    let back = at - 1.;
    back * back * back + 1.
}

pub fn in_out_sine(at: f32) -> f32 {
    (1. - (at * std::f32::consts::PI).cos()) / 2.
}

/// Overshoots and comes back. `over` is what Qt calls the overshoot.
pub fn out_back(at: f32, over: f32) -> f32 {
    let back = at - 1.;
    back * back * ((over + 1.) * back + over) + 1.
}

/// A value that goes up and comes back down for ever, on `period`.
fn breathe(now: u128, period: u128) -> f32 {
    let half = period / 2;
    let round = now % period;
    if round < half { in_out_sine(round as f32 / half as f32) } else { 1. - in_out_sine((round - half) as f32 / half as f32) }
}

// -- What the whole scene rides on -----------------------------------------

/// The state every act is drawn against.
pub struct Stage {
    pub now: u128,
    pub width: f32,
    pub height: f32,
    /// The height everything in acts one, two and five is hung from.
    pub middle_y: f32,
    /// Fades the picture in at the start and out at the end.
    pub reveal: f32,
    pub fading: f32,
    /// One clock, four things breathing on it.
    pub pulse: f32,
    pub cam: Cam,
}

/// When each act cut happened, for the flash and the pump.
fn last_cut(now: u128) -> Option<u128> {
    [FLAG, TANK, FACE, WAKE].into_iter().filter(|at| now >= *at).next_back()
}

fn shake_at(now: u128) -> (f32, f32) {
    for (at, amp) in acts::jolts() {
        if now < at || now > at + 420 {
            continue;
        }
        let through = through(now, at, 420);
        return (
            (through * 31.4).sin() * amp * (1. - through),
            (through * 23.6).sin() * amp * 0.6 * (1. - through),
        );
    }
    (0., 0.)
}

/// Where the camera stands for each act, and how it moves through it.
///
/// There used to be one move for the whole thing: a five per cent push in
/// over twenty-six seconds, the same from the first frame to the last. That
/// is not a camera, it is a zoom nobody asked for, and it is why five quite
/// different acts all felt like the same shot.
///
/// Each act gets its own now, and each begins where the last one left off
/// rather than snapping back, so the picture is always moving and never
/// jumps. In scale, and in how far it is pushed off centre as a fraction of
/// the picture.
fn shot(now: u128, width: f32, height: f32) -> (f32, (f32, f32)) {
    // (when it starts, how long the move takes, scale at the end, where it
    // ends up). The scale each act starts at is the one before it, so a cut
    // changes what is on screen without changing where the camera is.
    let moves: [(u128, u128, f32, (f32, f32)); 5] = [
        // The star: creeping in, dead centre, letting it hang there.
        (0, FLAG, 1.10, (0., 0.)),
        // The flag: drifting along the cloth, as though reading it.
        (FLAG, TANK - FLAG, 1.16, (-0.045, 0.01)),
        // The tank: pulled back to put the ground in, and tracking with it.
        (TANK, FACE - TANK, 0.98, (0.05, 0.035)),
        // The face: in, hard, and off the middle so it is not a portrait
        // shot but a look at somebody.
        (FACE, WAKE - FACE, 1.30, (-0.02, -0.02)),
        // The waking: the slowest of the five, still going at the end.
        (WAKE, TRACK - WAKE, 1.46, (0.015, 0.)),
    ];

    let mut scale = 1.;
    let mut at = (0., 0.);
    for (from, over, to_scale, to_at) in moves {
        if now <= from {
            break;
        }
        let along = in_out_sine(through(now, from, over));
        let (was_scale, was_at) = (scale, at);
        scale = was_scale + (to_scale - was_scale) * along;
        at = (was_at.0 + (to_at.0 - was_at.0) * along, was_at.1 + (to_at.1 - was_at.1) * along);
        if now < from + over {
            break;
        }
    }
    (scale, (at.0 * width, at.1 * height))
}

impl Stage {
    fn at(now: u128, bounds: Bounds<Pixels>) -> Stage {
        let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let drift = 1. + 0.05 * (now.min(PUSH_IN) as f32 / PUSH_IN as f32);
        let pump = match last_cut(now) {
            Some(cut) if now < cut + 130 => 1. + 0.03 * out_quad(through(now, cut, 130)),
            Some(cut) if now < cut + 680 => 1.03 - 0.03 * out_cubic(through(now, cut + 130, 550)),
            _ => 1.,
        };
        let (framing, framed_at) = shot(now, width, height);
        let shake = shake_at(now);
        let shift = (shake.0 + framed_at.0, shake.1 + framed_at.1);
        Stage {
            now,
            width,
            height,
            middle_y: height * 0.46,
            reveal: out_cubic(through(now, 0, 1400)),
            fading: 1. - in_quad(through(now, TRACK, FADE)),
            pulse: breathe(now, 3200),
            cam: Cam::new(bounds.origin, (width / 2., height / 2.), drift * pump * framing, shift),
        }
    }

    /// How bright the white between the acts is.
    fn flash(&self) -> f32 {
        match last_cut(self.now) {
            Some(cut) if self.now < cut + 100 => 0.85 * out_quad(through(self.now, cut, 100)),
            Some(cut) if self.now < cut + 600 => 0.85 * (1. - in_quad(through(self.now, cut + 100, 500))),
            _ => 0.,
        }
    }
}

impl Render for Cinema {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Nothing here is pressed; the pointer finds the desktop instead.
        window.set_input_region(Some(&[]));
        let now = self.at();

        rsx! {
            <div class="relative size-full">
                <canvas
                    class="absolute size-full"
                    prepaint={|_, _, _| ()}
                    paint={move |bounds: Bounds<Pixels>, _, window: &mut Window, cx: &mut App| {
                        let stage = Stage::at(now, bounds);
                        if stage.fading <= 0. {
                            return;
                        }
                        acts::paint(&stage, window, cx);
                        furniture(&stage, window);
                    }}
                />
            </div>
        }
    }
}

/// What sits in front of the picture and does not move with it: the wash of
/// dark over the desktop is inside the world, but these are not.
fn furniture(stage: &Stage, window: &mut Window) {
    let still = Cam::still(stage.cam.origin);
    let (width, height) = (stage.width, stage.height);
    let dim = stage.reveal * stage.fading;

    let vignette = height * 0.28;
    paint::rect(window, &still, (0., 0., width, vignette), 0., paint::down(shade(0x000000, 0.4 * dim), shade(0x000000, 0.)));
    paint::rect(
        window,
        &still,
        (0., height - vignette, width, vignette),
        0.,
        paint::down(shade(0x000000, 0.), shade(0x000000, 0.4 * dim)),
    );
    // The sides as well, which the old one never had: a picture darkened top
    // and bottom only is a picture in a letterbox, and one darkened all round
    // is a picture through a lens.
    let side = width * 0.16;
    for (x, from, to) in [(0., 0.34, 0.), (width - side, 0., 0.34)] {
        paint::rect(window, &still, (x, 0., side, height), 0.,
            paint::across(shade(0x000000, from * dim), shade(0x000000, to * dim)));
    }

    grain(stage, window, &still);

    // Bars that slide in from off the screen as the picture arrives.
    let bar = height * 0.085;
    paint::rect(window, &still, (0., -bar + bar * stage.reveal, width, bar), 0., shade(0x000000, stage.fading));
    paint::rect(window, &still, (0., height - bar * stage.reveal, width, bar), 0., shade(0x000000, stage.fading));

    let flash = stage.flash();
    if flash > 0. {
        paint::rect(window, &still, (0., 0., width, height), 0., shade(0xffffff, flash * stage.fading));
    }
}

/// Film grain, over everything and under nothing.
///
/// The QML had none: a thousand moving specks meant a thousand `Rectangle`s
/// or a shader, and neither was worth it there. Here it is a thousand paths
/// in one paint, worked out from the frame number, so it costs a loop and
/// nothing is kept between frames. It is most of what separates "shapes
/// drawn on a screen" from "something filmed".
fn grain(stage: &Stage, window: &mut Window, still: &Cam) {
    const SPECKS: usize = 900;
    // A new scatter roughly every other frame: grain that changes every
    // frame at sixty fizzes, and grain that never changes is dirt on the
    // lens.
    let roll = (stage.now / 33) as u32;
    let strength = 0.055 * stage.reveal * stage.fading;

    for speck in 0..SPECKS {
        let seed = (speck as u32).wrapping_mul(2_246_822_519).wrapping_add(roll.wrapping_mul(2_654_435_761));
        let mut state = seed | 1;
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let x = (state % 10_000) as f32 / 10_000. * stage.width;
        state ^= state << 7;
        let y = ((state >> 3) % 10_000) as f32 / 10_000. * stage.height;
        let bright = ((state >> 11) % 1_000) as f32 / 1_000.;
        // Mostly dark, occasionally a bright one, as film is.
        let (colour, alpha) = if bright > 0.82 { (0xffffff, strength * 1.5) } else { (0x000000, strength) };
        paint::rect(window, still, (x, y, 1.6, 1.6), 0., shade(colour, alpha * bright.max(0.35)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_acts_take_over_in_order_and_the_last_one_waits_for_the_track() {
        assert!(FLAG < TANK && TANK < FACE && FACE < WAKE);
        assert!(WAKE < TRACK, "the last act has the rest of the track to itself");
        assert_eq!(last_cut(0), None, "nothing has been cut to yet");
        assert_eq!(last_cut(FLAG), Some(FLAG));
        assert_eq!(last_cut(TANK - 1), Some(FLAG));
        assert_eq!(last_cut(TRACK), Some(WAKE));
    }

    #[test]
    fn the_camera_never_jumps_and_never_stops() {
        // Every act hands the camera to the next where it left it. If one
        // started from its own numbers instead, the cut would move the
        // camera as well as the picture, which reads as a glitch rather than
        // an edit — and it is invisible in a still, so it has to be counted.
        let (w, h) = (1920., 1080.);
        let mut worst = 0.;
        let mut still_for = 0;
        let mut longest_still = 0;
        let mut last = shot(0, w, h);
        for now in (16..TRACK).step_by(16) {
            let this = shot(now, w, h);
            let jump = (this.0 - last.0).abs() * 1000.
                + (this.1.0 - last.1.0).abs()
                + (this.1.1 - last.1.1).abs();
            if jump > worst {
                worst = jump;
            }
            // "Moving" is generous: a tenth of a pixel a frame is still a
            // camera, a flat zero for seconds on end is a held frame.
            if jump < 0.02 {
                still_for += 1;
                longest_still = longest_still.max(still_for);
            } else {
                still_for = 0;
            }
            last = this;
        }
        assert!(worst < 6., "the camera jumps by {worst} in one frame somewhere");
        assert!(longest_still < 90, "the camera sits perfectly still for {longest_still} frames");
    }

    #[test]
    fn every_act_is_framed_differently() {
        // Five acts that all sit at the same scale in the same place are
        // five acts that look like one shot, which is what this replaced.
        let (w, h) = (1920., 1080.);
        let framings: Vec<(f32, (f32, f32))> =
            [FLAG, TANK, FACE, WAKE, TRACK - 200].iter().map(|at| shot(at - 100, w, h)).collect();
        for (one, other) in framings.iter().zip(framings.iter().skip(1)) {
            let apart = (one.0 - other.0).abs() * 400. + (one.1.0 - other.1.0).abs() + (one.1.1 - other.1.1).abs();
            assert!(apart > 8., "two acts end up framed the same: {one:?} and {other:?}");
        }
    }

    #[test]
    fn the_flash_is_over_long_before_the_act_is() {
        let bright = |now| Stage { now, ..Stage::at(now, Bounds::new(point(px(0.), px(0.)), Size::new(px(1920.), px(1080.)))) }.flash();
        assert_eq!(bright(FLAG - 1), 0.);
        assert!(bright(FLAG + 100) > 0.8, "brightest a tenth of a second in");
        assert!(bright(FLAG + 400) > 0.);
        assert_eq!(bright(FLAG + 600), 0.);
    }

    #[test]
    fn the_picture_starts_where_it_is_and_is_well_inside_by_the_end() {
        // This used to say the whole piece was one five per cent push in and
        // that it stopped. It is five moves now, so what is worth holding is
        // the shape of the whole: it opens on the picture as it is, and it
        // has travelled a long way in by the time the track runs out.
        let scale = |now| Stage::at(now, Bounds::new(point(px(0.), px(0.)), Size::new(px(1920.), px(1080.)))).cam.scale;
        assert!((scale(0) - 1.).abs() < 0.001, "it opens where the desktop is");
        assert!(scale(TRACK) > 1.4, "by the end it is well in, got {}", scale(TRACK));
        assert!(scale(TRACK) < 1.9, "and not so far in that the picture is lost, got {}", scale(TRACK));
        // The slow global drift is still underneath all five, and still stops.
        assert!(scale(PUSH_IN * 2) > scale(TRACK) - 0.001);
    }

    #[test]
    fn a_jolt_dies_away_and_leaves_nothing_behind() {
        let (at, _) = acts::jolts()[0];
        assert_ne!(shake_at(at + 40), (0., 0.));
        assert_eq!(shake_at(at + 500), (0., 0.));
        assert_eq!(shake_at(0), (0., 0.));
    }

    #[test]
    fn nothing_is_drawn_once_the_track_has_faded_out() {
        let stage = Stage::at(TRACK + FADE, Bounds::new(point(px(0.), px(0.)), Size::new(px(1920.), px(1080.))));
        assert_eq!(stage.fading, 0.);
    }
}

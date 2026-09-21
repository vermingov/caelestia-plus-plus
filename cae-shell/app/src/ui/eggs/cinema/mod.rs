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
        ..Default::default()
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

impl Stage {
    fn at(now: u128, bounds: Bounds<Pixels>) -> Stage {
        let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let drift = 1. + 0.05 * (now.min(PUSH_IN) as f32 / PUSH_IN as f32);
        let pump = match last_cut(now) {
            Some(cut) if now < cut + 130 => 1. + 0.03 * out_quad(through(now, cut, 130)),
            Some(cut) if now < cut + 680 => 1.03 - 0.03 * out_cubic(through(now, cut + 130, 550)),
            _ => 1.,
        };
        let shift = shake_at(now);
        Stage {
            now,
            width,
            height,
            middle_y: height * 0.46,
            reveal: out_cubic(through(now, 0, 1400)),
            fading: 1. - in_quad(through(now, TRACK, FADE)),
            pulse: breathe(now, 3200),
            cam: Cam::new(bounds.origin, (width / 2., height / 2.), drift * pump, shift),
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

    // Bars that slide in from off the screen as the picture arrives.
    let bar = height * 0.085;
    paint::rect(window, &still, (0., -bar + bar * stage.reveal, width, bar), 0., shade(0x000000, stage.fading));
    paint::rect(window, &still, (0., height - bar * stage.reveal, width, bar), 0., shade(0x000000, stage.fading));

    let flash = stage.flash();
    if flash > 0. {
        paint::rect(window, &still, (0., 0., width, height), 0., shade(0xffffff, flash * stage.fading));
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
    fn the_flash_is_over_long_before_the_act_is() {
        let bright = |now| Stage { now, ..Stage::at(now, Bounds::new(point(px(0.), px(0.)), Size::new(px(1920.), px(1080.)))) }.flash();
        assert_eq!(bright(FLAG - 1), 0.);
        assert!(bright(FLAG + 100) > 0.8, "brightest a tenth of a second in");
        assert!(bright(FLAG + 400) > 0.);
        assert_eq!(bright(FLAG + 600), 0.);
    }

    #[test]
    fn the_picture_is_pushed_in_slowly_and_stops() {
        let scale = |now| Stage::at(now, Bounds::new(point(px(0.), px(0.)), Size::new(px(1920.), px(1080.)))).cam.scale;
        assert!((scale(0) - 1.).abs() < 0.001);
        assert!(scale(TRACK) > 1.03 && scale(TRACK) < 1.05);
        assert!((scale(PUSH_IN * 2) - 1.05).abs() < 0.001, "it stops where it was told to");
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

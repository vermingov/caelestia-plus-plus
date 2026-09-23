//! The other egg: five acts over a track, over the whole of one screen, and
//! nobody asked for any of it.
//!
//! It began as two thousand lines of QML `Rectangle`s and came across into
//! GPUI as the same flat shapes drawn with paths; it is drawn the way the
//! background's helix is now, on the card directly from a thread of its own
//! (`film`), as marks a shader works out a pixel at a time (`marks`,
//! `film.wgsl`): strokes of light that glow, motes out of focus, a flag of
//! real cloth, eyes with irises and lids, a night sky and fireworks. The
//! picture over the desktop is seen through, and a press goes through it to
//! whatever is underneath.
//!
//! Every act, every wobble and every shake is read off the time since the
//! first frame went, which is also when the track starts: there is no state
//! to get out of step with the music.

mod acts;
mod film;
mod kit;
mod marks;
mod portrait;

use gpui::{App, AppContext};

use marks::Cam;

/// When each act takes over, in milliseconds from the start. The first
/// begins at nought, with the surface itself.
pub const FLAG: u128 = 4200;
pub const SKY: u128 = 7800;
pub const FACE: u128 = 10600;
pub const WAKE: u128 = 13200;

/// How long the track is. The last act runs until it ends rather than to a
/// clock of its own.
pub const TRACK: u128 = 17_984;
/// What the fade out takes, and how long after it the surface goes.
const FADE: u128 = 1200;
pub const OVER: u128 = TRACK + 1300;

/// A slow push in under all five acts' own camera moves.
const PUSH_IN: u128 = 26_000;

/// What Quickshell is asked about before it plays: the same piece as the
/// desktop egg.
const PIECE: &str = "egg";

/// Plays it on the screen somebody is looking at, or asks the old shell to
/// while the old shell still draws it. Once at a time.
pub fn pop(cx: &mut App) {
    crate::ours::when_known(PIECE, cx, |ours, cx| {
        if !ours {
            return cx.background_spawn(async { drop(cae_core::services::ipc("israelEgg", "pop", &[])) }).detach();
        }
        film::play();
    });
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

/// The state every act is laid out against.
pub struct Stage {
    pub now: u128,
    /// The screen, in its own pixels.
    pub width: f32,
    pub height: f32,
    /// How many of those pixels one of the compositor's units is: sizes
    /// that are not a share of the screen are written in the latter.
    pub unit: f32,
    /// The height everything in acts one, two and five is hung from.
    pub middle_y: f32,
    /// Fades the picture in at the start and out at the end.
    pub reveal: f32,
    pub fading: f32,
    /// One clock, several things breathing on it.
    pub pulse: f32,
    pub cam: Cam,
}

/// When each act cut happened, for the flash and the pump.
fn last_cut(now: u128) -> Option<u128> {
    [FLAG, SKY, FACE, WAKE].into_iter().rfind(|at| now >= *at)
}

fn shake_at(now: u128) -> (f32, f32) {
    for (at, amp) in acts::jolts() {
        if now < at || now > at + 420 {
            continue;
        }
        let through = through(now, at, 420);
        return ((through * 31.4).sin() * amp * (1. - through), (through * 23.6).sin() * amp * 0.6 * (1. - through));
    }
    (0., 0.)
}

/// Where the camera stands for each act, and how it moves through it.
///
/// Each act gets its own move, and each begins where the last one left off
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
        (FLAG, SKY - FLAG, 1.16, (-0.045, 0.01)),
        // The night: pulled back and lifted, to take in the sky.
        (SKY, FACE - SKY, 0.98, (0.02, 0.045)),
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
    pub fn at(now: u128, (width, height): (f32, f32), unit: f32) -> Stage {
        let drift = 1. + 0.05 * (now.min(PUSH_IN) as f32 / PUSH_IN as f32);
        let pump = match last_cut(now) {
            Some(cut) if now < cut + 130 => 1. + 0.03 * out_quad(through(now, cut, 130)),
            Some(cut) if now < cut + 680 => 1.03 - 0.03 * out_cubic(through(now, cut + 130, 550)),
            _ => 1.,
        };
        let (framing, framed_at) = shot(now, width, height);
        let shake = shake_at(now);
        let shift = (shake.0 * unit + framed_at.0, shake.1 * unit + framed_at.1);
        Stage {
            now,
            width,
            height,
            unit,
            middle_y: height * 0.46,
            reveal: out_cubic(through(now, 0, 1400)),
            fading: 1. - in_quad(through(now, TRACK, FADE)),
            pulse: breathe(now, 3200),
            cam: Cam::zoom((width / 2., height / 2.), drift * pump * framing, shift),
        }
    }

    pub fn screen(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    /// How bright the white between the acts is.
    pub fn flash(&self) -> f32 {
        match last_cut(self.now) {
            Some(cut) if self.now < cut + 100 => 0.85 * out_quad(through(self.now, cut, 100)),
            Some(cut) if self.now < cut + 600 => 0.85 * (1. - in_quad(through(self.now, cut + 100, 500))),
            _ => 0.,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(now: u128) -> Stage {
        Stage::at(now, (1920., 1080.), 1.)
    }

    #[test]
    fn the_acts_take_over_in_order_and_the_last_one_waits_for_the_track() {
        assert!(FLAG < SKY && SKY < FACE && FACE < WAKE);
        assert!(WAKE < TRACK, "the last act has the rest of the track to itself");
        assert_eq!(last_cut(0), None, "nothing has been cut to yet");
        assert_eq!(last_cut(FLAG), Some(FLAG));
        assert_eq!(last_cut(SKY - 1), Some(FLAG));
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
            let jump = (this.0 - last.0).abs() * 1000. + (this.1.0 - last.1.0).abs() + (this.1.1 - last.1.1).abs();
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
        let (w, h) = (1920., 1080.);
        let framings: Vec<(f32, (f32, f32))> = [FLAG, SKY, FACE, WAKE, TRACK - 200].iter().map(|at| shot(at - 100, w, h)).collect();
        for (one, other) in framings.iter().zip(framings.iter().skip(1)) {
            let apart = (one.0 - other.0).abs() * 400. + (one.1.0 - other.1.0).abs() + (one.1.1 - other.1.1).abs();
            assert!(apart > 8., "two acts end up framed the same: {one:?} and {other:?}");
        }
    }

    #[test]
    fn the_flash_is_over_long_before_the_act_is() {
        assert_eq!(stage(FLAG - 1).flash(), 0.);
        assert!(stage(FLAG + 100).flash() > 0.8, "brightest a tenth of a second in");
        assert!(stage(FLAG + 400).flash() > 0.);
        assert_eq!(stage(FLAG + 600).flash(), 0.);
    }

    #[test]
    fn the_picture_starts_where_it_is_and_is_well_inside_by_the_end() {
        let scale = |now| stage(now).cam.scale();
        assert!((scale(0) - 1.).abs() < 0.001, "it opens where the desktop is");
        assert!(scale(TRACK) > 1.4, "by the end it is well in, got {}", scale(TRACK));
        assert!(scale(TRACK) < 1.9, "and not so far in that the picture is lost, got {}", scale(TRACK));
    }

    #[test]
    fn a_jolt_dies_away_and_leaves_nothing_behind() {
        // The last, the clamps landing, with nothing after it to overlap.
        let (at, _) = *acts::jolts().last().expect("jolts");
        assert_ne!(shake_at(at + 40), (0., 0.));
        assert_eq!(shake_at(at + 500), (0., 0.));
        assert_eq!(shake_at(0), (0., 0.));
    }

    #[test]
    fn nothing_is_drawn_once_the_track_has_faded_out() {
        assert_eq!(stage(TRACK + FADE).fading, 0.);
        assert!(OVER > TRACK + FADE, "the surface goes only once the picture has");
    }
}

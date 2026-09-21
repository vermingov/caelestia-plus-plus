//! The five acts, in the order they play.
//!
//! Every shape here is the shape the QML had, at the same fraction of the
//! screen and in the same colour. What was a binding on `phase` is a reading
//! off the clock; what was a `Behavior` is the curve it named, started at
//! the moment the act it belonged to began.

use gpui::{App, Window, px};

use super::super::paint::{self, Cam, shade};
use super::super::sticker;
use super::{FACE, FLAG, Stage, TANK, WAKE, in_cubic, in_out_quad, in_out_sine, out_back, out_cubic, out_quad, through};

// The colours, once, by what they are rather than what they are made of.
const INK: u32 = 0xf4f7ff;
const BLUE: u32 = 0x3d6bff;
const FLAG_BLUE: u32 = 0x0038b8;
const CLOTH: u32 = 0xfdfdfd;
const CLOTH_EDGE: u32 = 0xc9d4e8;
const IRIS: u32 = 0x2e6bd6;
const PUPIL: u32 = 0x101318;
const SKIN: u32 = 0xe8c7a2;

/// Everything that shakes the picture, and how hard. The tank's five are
/// worked out from where each one is standing and how far along the drive
/// the hull reaches them.
pub fn jolts() -> Vec<(u128, f32)> {
    let mut all: Vec<(u128, f32)> = (0..5)
        .map(|index| {
            let foot = 0.12 + index as f32 * 0.13;
            // The hull's leading edge is at `1 - drive * 1.28` of the width.
            let drive = (1. - foot) / 1.28;
            (TANK + undo_in_out_sine(drive) as u128, 3.5)
        })
        .collect();
    // The clamps landing, which is the one that is meant to be felt.
    all.push((WAKE + 1480, 9.));
    all
}

/// How long into the drive a given fraction of it is reached, in
/// milliseconds: the drive is 2750 ms of `InOutSine`, and this is that curve
/// read backwards.
fn undo_in_out_sine(part: f32) -> f32 {
    2750. * (1. - 2. * part.clamp(0., 1.)).acos() / std::f32::consts::PI
}

/// A value that swings between `-amp` and `+amp` for ever, one way in
/// `half`, the other in `half`.
fn sway(now: u128, half: u128, amp: f32) -> f32 {
    let round = now % (half * 2);
    let at = if round < half {
        in_out_sine(round as f32 / half as f32)
    } else {
        1. - in_out_sine((round - half) as f32 / half as f32)
    };
    (at * 2. - 1.) * amp
}

pub fn paint(stage: &Stage, window: &mut Window, cx: &mut App) {
    let dim = stage.fading;
    // The wash of dark over the desktop, which the whole picture sits in.
    paint::rect(window, &stage.cam, (0., 0., stage.width, stage.height), 0., shade(0x050d26, stage.reveal * 0.55 * dim));
    sparks(stage, window);
    rays(stage, window);
    flag(stage, window);
    star(stage, window);
    portrait(stage, window);
    eye_rain(stage, window);
    tank_scene(stage, window, cx);
    awakening(stage, window, cx);
}

// -- Behind everything -----------------------------------------------------

/// Motes drifting up, all the way through.
fn sparks(stage: &Stage, window: &mut Window) {
    // Three planes rather than one. The far ones are small, slow, dim and
    // barely move across; the near ones are large, quick and wander. It is
    // the same loop three times over with different numbers, which is all
    // depth ever is, and the reason the old one read as a flat sheet of
    // motes is that every one of them was the same distance away.
    const PLANES: [(usize, u128, f32, f32, f32); 3] =
        [(34, 11_000, 0.55, 0.26, 5.), (24, 7_000, 1.0, 0.55, 14.), (12, 4_600, 1.7, 0.85, 26.)];

    for (plane, &(count, period, size, strength, wobble)) in PLANES.iter().enumerate() {
        let clock = (stage.now % period) as f32 / period as f32;
        let salt = plane as u32 * 977;
        for index in 0..count {
            let lane = wander(index + plane * 71, 1 + salt);
            let drift = wander(index + plane * 71, 2 + salt);
            let at = (clock + drift) % 1.;
            let x = lane * stage.width + (at * 12.6 + drift * 9.).sin() * wobble;
            let y = stage.height * (1. - at);
            let radius = (1.4 + (index % 3) as f32 * 0.9) * size;
            let colour = if index % 4 == 0 { 0x7d9bff } else { INK };
            let alpha = (at * std::f32::consts::PI).sin() * 0.7 * strength * stage.reveal * stage.fading;
            // The near ones carry a little of their own light, which is what
            // stops a bright mote looking like a sticker of a bright mote.
            if plane == 2 {
                paint::glow(window, &stage.cam, x, y, radius * 4.5, colour, alpha * 0.5);
            }
            paint::disc(window, &stage.cam, x, y, radius, 1., shade(colour, alpha));
        }
    }
}

/// A number between nought and one that is always the same for the same
/// thing. The QML rolled these once per delegate; this rolls them once per
/// index, which comes to the same picture without keeping anything.
fn wander(index: usize, salt: u32) -> f32 {
    let mut state = (index as u32).wrapping_mul(2_654_435_761).wrapping_add(salt.wrapping_mul(97_531)) | 1;
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    (state % 10_000) as f32 / 10_000.
}

/// Two fans of light turning against each other behind the whole thing.
fn rays(stage: &Stage, window: &mut Window) {
    let y = if stage.now >= FACE {
        let to = in_out_quad(through(stage.now, FACE, 800));
        stage.middle_y + (stage.height * 0.42 - stage.middle_y) * to
    } else {
        stage.middle_y
    };
    let alpha = stage.reveal * stage.fading * if stage.now >= FACE { 0.2 } else { 0.14 };
    let reach = stage.width.min(stage.height);
    let fan = |window: &mut Window, count: usize, period: u128, width: f32, length: f32, turn_back: bool, strength: f32| {
        let round = (stage.now % period) as f32 / period as f32;
        let turn = if turn_back { -round } else { round };
        for beam in 0..count {
            let at = turn + beam as f32 / count as f32;
            let cam = stage.cam.turned((stage.width / 2., y), at);
            // Bright at the far end, gone at the middle: the gradient the
            // QML beam had, top to bottom, turned on its head by the anchor.
            paint::rect(
                window,
                &cam,
                (stage.width / 2. - width / 2., y - length, width, length),
                0.,
                paint::down(shade(0xdce6ff, 0.), shade(0xdce6ff, alpha * strength)),
            );
        }
    };
    fan(window, 8, 40_000, 3., reach * 0.68, false, 1.);
    fan(window, 6, 61_000, 2., reach * 0.5, true, 0.6);
}

// -- Act one: the star -----------------------------------------------------

/// The six points, as two triangles. `start` is where the first corner is,
/// in full turns from straight up.
fn triangle(width: f32, start: f32) -> [(f32, f32); 4] {
    let (radius, centre) = (width / 2. / 1.2, width / 2.);
    std::array::from_fn(|corner| {
        let angle = (start + corner as f32 / 3.) * std::f32::consts::TAU;
        (centre + radius * angle.cos(), centre + radius * angle.sin())
    })
}

/// The star itself, drawn into a box `width` across at `(x, y)`.
fn magen_david(window: &mut Window, cam: &Cam, (x, y): (f32, f32), width: f32, ink: u32, glow: u32, pulse: f32, alpha: f32) {
    let at = cam.about((x, y), 0., 1., 1.);
    let shift = |points: [(f32, f32); 4]| points.map(|(px_, py)| (x + px_, y + py));
    let (up, down) = (shift(triangle(width, -0.25)), shift(triangle(width, 0.25)));
    // The soft wide pass first, then the thin bright one over it.
    for (weight, colour, strength) in [(width * 0.084, glow, 0.25 + pulse * 0.2), (width * 0.028, ink, 1.)] {
        for points in [&up, &down] {
            paint::line(window, &at, points, weight, shade(colour, strength * alpha));
        }
    }
}

fn star(stage: &Stage, window: &mut Window) {
    let showing = stage.now < FLAG;
    let alpha = stage.reveal * stage.fading * if showing { 1. } else { 0. };
    if alpha <= 0. {
        return;
    }
    let width = stage.width.min(stage.height) * 0.5;
    let (x, y) = (stage.width / 2. - width / 2., stage.middle_y - width / 2.);

    paint::glow(window, &stage.cam, stage.width / 2., stage.middle_y, width * 1.05, BLUE, 0.5 * alpha * (0.45 + stage.pulse * 0.35));

    // A ring thrown outwards once, thinning as it goes.
    let burst = out_cubic(through(stage.now, 0, 1600));
    let ring = width * (0.5 + burst * 1.3);
    paint::rect_edge(
        window,
        &stage.cam,
        (stage.width / 2. - ring / 2., stage.middle_y - ring / 2., ring, ring),
        ring / 2.,
        4f32.mul_add(-burst, 4.).max(1.5),
        shade(INK, (1. - burst) * 0.7 * alpha),
    );

    let arrive = out_back(through(stage.now, 0, 1500), 1.4);
    let scale = (0.55 + 0.45 * arrive) * (0.96 + stage.pulse * 0.05);
    let turn = ((1. - arrive) * -12. + sway(stage.now, 2400, 2.5)) / 360.;
    let cam = stage.cam.about((stage.width / 2., stage.middle_y), turn, scale, scale);
    magen_david(window, &cam, (x, y), width, INK, BLUE, stage.pulse, alpha);
}

// -- Act two: the flag -----------------------------------------------------

fn flag(stage: &Stage, window: &mut Window) {
    // Up twice: its own act, then small and high over the portrait.
    let alpha = stage.fading
        * match stage.now {
            now if now < FLAG => 0.,
            now if now < TANK => through(now, FLAG, 500),
            now if now < FACE => 1. - through(now, TANK, 500),
            now if now < WAKE => through(now, FACE, 500),
            now => 1. - through(now, WAKE, 500),
        };
    if alpha <= 0.001 {
        return;
    }

    let width = stage.width * 0.46;
    let height = width * 8. / 11.;
    // Off the left edge until its act, then swept in past the mark and back.
    let from = -width * 1.6 - width / 2.;
    let to = stage.width / 2. - width / 2.;
    let x = from + (to - from) * out_back(through(stage.now, FLAG, 900), 1.1);
    let lifted = in_out_quad(through(stage.now, FACE, 800));
    let middle_y = stage.middle_y + (stage.height * 0.17 - stage.middle_y) * lifted;
    let scale = 1. - 0.5 * lifted;
    let y = middle_y - height / 2.;

    // Both turns are pinned at the pole, one in the plane and one about it.
    let hoist = (x, y + height / 2.);
    let flutter = sway(stage.now, 760, 7.) / 360.;
    let cam = stage
        .cam
        .about((x + width / 2., middle_y), 0., scale, scale)
        .turned(hoist, sway(stage.now, 1400, 2.5) / 360.)
        .about(hoist, 0., (flutter * std::f32::consts::TAU).cos(), 1.);

    paint::glow(window, &cam, x + width / 2., y + height / 2., width * 0.75, 0xbcd0ff, 0.22 * alpha);

    // The pole, and the knob on top of it.
    let pole_w = (width * 0.016f32).max(4.);
    let pole_x = x - pole_w - width * 0.015;
    paint::rect(
        window,
        &cam,
        (pole_x, y - height * 0.14, pole_w, height * 1.34),
        pole_w / 2.,
        paint::down(shade(0xe8edf5, alpha), shade(0x98a2b3, alpha)),
    );
    let knob = pole_w * 2.6;
    paint::disc(window, &cam, pole_x + pole_w / 2., y - height * 0.14 - knob * 0.2, knob / 2., 1., shade(0xf2d27c, alpha));

    paint::rect(window, &cam, (x, y, width, height), 6., shade(CLOTH, alpha));
    paint::rect_edge(window, &cam, (x, y, width, height), 6., 1., shade(CLOTH_EDGE, alpha));

    let inset = width * 0.02;
    for band in [0.14, 0.75] {
        paint::rect(window, &cam, (x + inset, y + height * band, width - inset * 2., height * 0.11), 0., shade(FLAG_BLUE, alpha));
    }

    let star_w = height * 0.5;
    magen_david(
        window,
        &cam,
        (x + width / 2. - star_w / 2., y + height / 2. - star_w / 2.),
        star_w,
        FLAG_BLUE,
        FLAG_BLUE,
        stage.pulse,
        alpha,
    );

    // A band of light that crosses it, over and over, in its own act only.
    if stage.now < TANK {
        let round = (stage.now - FLAG) % 2800;
        if round < 1900 {
            let sweep = in_out_quad(round as f32 / 1900.);
            let band = width * 0.3;
            let sheen = x - band + sweep * (width + band * 2.);
            let cam = cam.turned((sheen + band / 2., y + height / 2.), 14. / 360.);
            paint::rect(
                window,
                &cam,
                (sheen, y - height * 0.25, band, height * 1.5),
                0.,
                paint::down(shade(0xffffff, 0.), shade(0xffffff, 0.19 * alpha)),
            );
        }
    }
}

// -- Act three: the tank ---------------------------------------------------

fn tank_scene(stage: &Stage, window: &mut Window, cx: &mut App) {
    let alpha = stage.fading
        * match stage.now {
            now if now < TANK => 0.,
            now if now < FACE => stage.reveal * through(now, TANK, 500),
            now => stage.reveal * (1. - through(now, FACE, 500)),
        };
    if alpha <= 0.001 {
        return;
    }
    let (w, h) = (stage.width, stage.height);
    let ground = h * 0.72;
    let tank_w = w * 0.28;
    let tank_h = tank_w * 0.46;
    let drive = in_out_sine(through(stage.now, TANK, 2750));
    let front = w - drive * (w + tank_w);
    let rear = front + tank_w;
    let cam = &stage.cam;

    // What is left standing behind it.
    for block in 0..6usize {
        let tall = tank_h * (0.35 + ((block * 2) % 3) as f32 * 0.22);
        let wide = w * (0.05 + (block % 2) as f32 * 0.03);
        let at = w * (0.02 + block as f32 * 0.16);
        paint::rect(window, cam, (at, ground - tall, wide, tall), 0., shade(0x141b2c, 0.55 * alpha));
        paint::rect(window, cam, (at + wide * 0.6, ground - tall * 1.25, wide * 0.35, tall * 1.25), 0., shade(0x141b2c, 0.55 * alpha));
    }

    paint::rect(window, cam, (0., ground, w, h - ground), 0., paint::down(shade(0x3a3326, alpha), shade(0x221d15, alpha)));
    for stone in 0..8usize {
        let size = 8. + (stone % 3) as f32 * 6.;
        let at = w * (0.03 + stone as f32 * 0.127 + (stone % 3) as f32 * 0.02);
        let up = ground + 8. + (stone % 4) as f32 * (h - ground) * 0.2;
        paint::rect(window, cam, (at, up, size, size * 0.45), 2., shade(0x171310, 0.8 * alpha));
    }
    for mark in [6., 13.] {
        paint::rect(window, cam, (rear, ground + mark, (w - rear).max(0.), 3.), 0., shade(0x151109, 0.7 * alpha));
    }

    gaza_sign(stage, window, cx, ground, front, alpha);
    for index in 0..5usize {
        victim(stage, window, ground, front, drive, index, alpha);
    }
    dust(stage, window, ground, rear, tank_h, alpha);
    tank(stage, window, cx, ground, front, drive, tank_w, tank_h, alpha);

    // The haze the whole act is seen through.
    paint::rect(window, cam, (0., 0., w, h), 0., paint::down(shade(0xb98d54, 0.1 * alpha), shade(0xb98d54, 0.25 * alpha)));
}

fn gaza_sign(stage: &Stage, window: &mut Window, cx: &mut App, ground: f32, front: f32, alpha: f32) {
    let (w, h) = (stage.width, stage.height);
    let board_w = w * 0.14;
    let board_h = board_w * 0.5;
    let x = w * 0.76;
    let tall = ground - h * 0.34;
    let y = ground - tall;

    // Knocked askew as the hull reaches it, and settling back.
    let bumped = front <= x + board_w * 0.5;
    let turn = if bumped {
        let since = stage.now.saturating_sub(bump_at(stage, x + board_w * 0.5));
        match since {
            since if since < 90 => -9. * out_quad(since as f32 / 90.),
            since if since < 240 => -9. + 13. * ((since - 90) as f32 / 150.),
            since if since < 370 => 4. - 6. * ((since - 240) as f32 / 130.),
            since if since < 550 => -2. + 2. * out_quad((since - 370) as f32 / 180.),
            _ => 0.,
        }
    } else {
        0.
    };
    let cam = stage.cam.turned((x + board_w / 2., ground), turn / 360.);

    let post_w = (board_w * 0.06f32).max(4.);
    paint::rect(window, &cam, (x + board_w / 2. - post_w / 2., y + board_h, post_w, tall - board_h), 0., shade(0x8a8f96, alpha));
    paint::rect(window, &cam, (x, y, board_w, board_h), board_h * 0.12, shade(0x0f7a3d, alpha));
    paint::rect_edge(window, &cam, (x, y, board_w, board_h), board_h * 0.12, (board_w * 0.03f32).max(2.), shade(0xffffff, alpha));
    paint::word(window, cx, &cam, "Gaza", (x + board_w / 2., y + board_h * 0.22), board_h * 0.42, shade(0xffffff, alpha), true);
}

/// When the hull's leading edge reaches `mark`, as a time from the start.
fn bump_at(stage: &Stage, mark: f32) -> u128 {
    let _ = stage;
    let part = (1. - mark / stage.width) / 1.28;
    TANK + undo_in_out_sine(part) as u128
}

fn victim(stage: &Stage, window: &mut Window, ground: f32, front: f32, drive: f32, index: usize, alpha: f32) {
    const SHIRTS: [u32; 5] = [0x9c5a3c, 0x5a6b8c, 0x7a8560, 0x8c5a75, 0xa08a4a];
    let (w, h) = (stage.width, stage.height);
    let foot = w * (0.12 + index as f32 * 0.13);
    let tall = h * (0.22 + (index % 3) as f32 * 0.03);
    let wide = tall * 0.42;
    let crushed = front <= foot;
    let panicked = !crushed && drive > 0. && (front - foot) < w * 0.14;

    // Flattened over a seventh of a second, and a ring of dust thrown out.
    let since = stage.now.saturating_sub(bump_at(stage, foot));
    let squash = if crushed { out_quad(through(stage.now, bump_at(stage, foot), 140)) } else { 0. };
    if crushed && since < 450 {
        let ring = out_quad(since as f32 / 450.);
        let puff_w = wide * 1.7 * (0.3 + ring * 0.9);
        paint::rect_edge(
            window,
            &stage.cam,
            (foot + wide / 2. - puff_w / 2., ground - puff_w * 0.25, puff_w, puff_w * 0.5),
            puff_w * 0.25,
            2.,
            shade(0xb9a98c, (1. - ring) * 0.8 * alpha),
        );
    }

    let lean = if panicked { -7. } else { 0. };
    let cam = stage
        .cam
        .turned((foot + wide / 2., ground), lean / 360.)
        .about((foot + wide / 2., ground), 0., 1. + 0.5 * squash, 1. - 0.9 * squash);
    let top = ground - tall;
    let shirt = shade(SHIRTS[index], alpha);

    for leg in [0.18, 0.58] {
        paint::rect(window, &cam, (foot + wide * leg, ground - tall * 0.24, wide * 0.2, tall * 0.24), wide * 0.1, shade(0x2e3138, alpha));
    }
    paint::rect(window, &cam, (foot + wide * 0.17, top + tall * 0.3, wide * 0.66, tall * 0.5), wide * 0.23, shirt);
    // Arms go up when there is something to put them up about.
    for (side, at) in [(1., 0.08), (-1., 0.79)] {
        let turn = if panicked { 150. * side } else { 12. * side };
        let arm = cam.turned((foot + wide * at + wide * 0.065, top + tall * 0.34), turn / 360.);
        paint::rect(window, &arm, (foot + wide * at, top + tall * 0.34, wide * 0.13, tall * 0.34), wide * 0.065, shirt);
    }
    let head = wide * 0.56;
    paint::disc(window, &cam, foot + wide / 2., top + head / 2., head / 2., 1., shade(SKIN, alpha));
}

/// The cloud it leaves behind, thinning as it lifts.
fn dust(stage: &Stage, window: &mut Window, ground: f32, rear: f32, tank_h: f32, alpha: f32) {
    for puff in 0..9usize {
        let size = tank_h * (0.75 + (puff % 3) as f32 * 0.3);
        let at = stage.width * (0.04 + puff as f32 * 0.115);
        let age = ((at - rear) / (stage.width * 0.28)).clamp(0., 1.);
        if age <= 0. {
            continue;
        }
        let up = ground - size * 0.55 - age * tank_h * 0.25;
        let strength = (age / 0.12).min((1. - age) / 0.88).max(0.) * 0.95 * alpha;
        paint::glow(window, &stage.cam, at, up, size / 2. * (0.5 + age * 1.3), 0xc4a26e, 0.85 * strength);
    }
}

#[allow(clippy::too_many_arguments)]
fn tank(stage: &Stage, window: &mut Window, cx: &mut App, ground: f32, front: f32, drive: f32, w: f32, h: f32, alpha: f32) {
    let jolt_a = (drive * std::f32::consts::PI * 16.).sin();
    let jolt_b = (drive * std::f32::consts::PI * 7. + 1.3).sin();
    let y = ground - h - (jolt_a.abs() * 0.6 + jolt_b.abs() * 0.4) * h * 0.04;
    let cam = stage.cam.turned((front, ground), (jolt_a * 0.8 + jolt_b * 0.5) / 360.);
    let at = |fx: f32, fy: f32| (front + w * fx, y + h * fy);

    paint::rect(window, &cam, (front + w * 0.02, y + h * 0.7, w * 0.97, h * 0.3), h * 0.15, shade(0x1c1c1c, alpha));
    for link in 0..12usize {
        let along = ((link as f32 / 12. + drive * 10.) % 1.) * 0.9;
        paint::rect(window, &cam, (front + w * (0.03 + along), y + h * 0.93, w * 0.045, h * 0.05), 2., shade(0x0d0d0d, alpha));
    }
    for wheel in 0..6usize {
        let size = h * 0.24;
        let x = front + w * 0.08 + wheel as f32 * (w * 0.8 / 5.);
        let mid = (x, y + h - size / 2.);
        let spun = cam.turned(mid, -drive * 2800. / 360.);
        paint::disc(window, &spun, mid.0, mid.1, size / 2., 1., shade(0x3a3a3a, alpha));
        paint::rect_edge(
            window,
            &spun,
            (mid.0 - size / 2., mid.1 - size / 2., size, size),
            size / 2.,
            (size * 0.12f32).max(1.),
            shade(0x111111, alpha),
        );
        paint::rect(window, &spun, (mid.0 - size * 0.37, mid.1 - size * 0.06, size * 0.74, size * 0.12), size * 0.06, shade(0x585858, alpha));
        paint::rect(window, &spun, (mid.0 - size * 0.06, mid.1 - size * 0.37, size * 0.12, size * 0.74), size * 0.06, shade(0x585858, alpha));
        paint::disc(window, &spun, mid.0, mid.1, size * 0.13, 1., shade(0x666666, alpha));
    }
    paint::rect(window, &cam, (front + w * 0.03, y + h * 0.56, w * 0.94, h * 0.18), h * 0.05, shade(0x3e4520, alpha));

    // The gun, which rides the rocking and never once goes off.
    let barrel = cam.turned((front + w * 0.34, y + h * 0.25), jolt_a * 0.7 / 360.);
    paint::rect(window, &barrel, (front - w * 0.38, y + h * 0.22, w * 0.72, h * 0.06), h * 0.03, shade(0x2b2f22, alpha));
    paint::rect(window, &barrel, (front - w * 0.38, y + h * 0.205, w * 0.05, h * 0.09), 2., shade(0x242817, alpha));

    let hull = [at(0., 0.52), at(0.1, 0.36), at(0.88, 0.34), at(1., 0.44), at(0.97, 0.62), at(0.05, 0.62)];
    paint::shape(window, &cam, &hull, shade(0x4b5320, alpha), Some((2., shade(0x333a18, alpha))));
    let turret = [at(0.3, 0.36), at(0.36, 0.16), at(0.64, 0.14), at(0.74, 0.28), at(0.78, 0.36)];
    paint::shape(window, &cam, &turret, shade(0x59653c, alpha), Some((2., shade(0x3b421e, alpha))));
    paint::rect(window, &cam, (front + w * 0.46, y + h * 0.1, w * 0.1, h * 0.05), h * 0.025, shade(0x3b421e, alpha));
    paint::word(window, cx, &cam, "IDF", at(0.44, 0.4), h * 0.16, shade(0xe6ead2, alpha), false);

    let mast = cam.turned((front + w * 0.9, y + h * 0.34), (14. + jolt_a * 3.) / 360.);
    paint::rect(window, &mast, (front + w * 0.9 - 1.5, y + h * 0.34 - h * 0.5, 3., h * 0.5), 1., shade(0x222519, alpha));
    let pennant = (front + w * 0.9 + 2., y + h * 0.34 - h * 0.5, w * 0.09, h * 0.07);
    paint::rect(window, &mast, pennant, 0., shade(CLOTH, alpha));
    paint::rect(window, &mast, (pennant.0, pennant.1 + pennant.3 * 0.35, pennant.2, pennant.3 * 0.3), 0., shade(FLAG_BLUE, alpha));
}

// -- Act four: the portrait ------------------------------------------------

fn portrait(stage: &Stage, window: &mut Window) {
    let alpha = stage.fading
        * match stage.now {
            now if now < FACE => 0.,
            now if now < WAKE => through(now, FACE, 600),
            now => 1. - through(now, WAKE, 600),
        };
    if alpha <= 0.001 {
        return;
    }
    let height = stage.height * 0.52;
    let width = height * 0.8;
    let rise = if stage.now < WAKE {
        out_back(through(stage.now, FACE, 900), 1.2)
    } else {
        1. - out_back(through(stage.now, WAKE, 900), 1.2)
    };
    let x = stage.width / 2. - width / 2.;
    let y = stage.height - height * rise;
    let foot = (stage.width / 2., y + height);
    let scale = 1. + stage.pulse * 0.03;
    let cam = stage.cam.turned(foot, sway(stage.now, 1100, 2.5) / 360.).about(foot, 0., scale, scale);

    paint::glow(window, &stage.cam, stage.width / 2., y + height / 2., height * 0.7, 0x4d79ff, 0.35 * alpha * 0.8);
    // Drawn where the QML loaded an SVG: GPUI will not turn one, and half
    // of this act is the turning.
    sticker::paint(
        &sticker::PORTRAIT,
        sticker::Placed { at: gpui::point(px(x + width / 2.), px(y + height)), about: (120., 300.), turn: 0., scale: width / 240., alpha },
        &cam,
        window,
    );
}

/// Eyes falling past, every one of them watching the middle of the screen.
fn eye_rain(stage: &Stage, window: &mut Window) {
    let alpha = stage.fading
        * match stage.now {
            now if now < FACE => 0.,
            now if now < WAKE => through(now, FACE, 600),
            now => 1. - through(now, WAKE, 600),
        };
    if alpha <= 0.001 {
        return;
    }
    let clock = ((stage.now - FACE) % 3400) as f32 / 3400.;
    for index in 0..22usize {
        let lane = wander(index, 3);
        let drop = wander(index, 4);
        let at = (clock + drop) % 1.;
        let width = 30. + (index % 3) as f32 * 8.;
        let height = width * 0.6;
        let x = lane * stage.width + (at * 12.6 + drop * 6.3).sin() * 30.;
        let y = -height + (stage.height + height * 2.) * at;
        let showing = (at * std::f32::consts::PI).sin().mul_add(1.8, 0.).min(1.) * alpha;
        if showing <= 0. {
            continue;
        }
        // A blink is the whole thing squashed flat for a moment.
        let blink = ((at * 18.8 + drop * 40.).sin() - 0.86).max(0.) / 0.14;
        let mid = (x + width / 2., y + height / 2.);
        let cam = stage
            .cam
            .turned(mid, (at * 6.3 + drop * 6.3).sin() * 16. / 360.)
            .about(mid, 0., 1., 1. - blink * 0.85);

        paint::rect(window, &cam, (x, y, width, height), height / 2., shade(CLOTH, showing));
        paint::rect_edge(window, &cam, (x, y, width, height), height / 2., 1., shade(CLOTH_EDGE, showing));
        let gaze = ((stage.width / 2. - mid.0) / (stage.width / 2.)).clamp(-1., 1.);
        let iris = height * 0.31;
        paint::disc(window, &cam, mid.0 + gaze * width * 0.14, mid.1, iris, 1., shade(IRIS, showing));
        paint::disc(window, &cam, mid.0 + gaze * width * 0.14, mid.1, iris * 0.45, 1., shade(PUPIL, showing));
    }
}

// -- Act five: the waking --------------------------------------------------

fn awakening(stage: &Stage, window: &mut Window, cx: &mut App) {
    if stage.now < WAKE {
        return;
    }
    let alpha = stage.reveal * stage.fading * through(stage.now, WAKE, 600);
    if alpha <= 0.001 {
        return;
    }
    let since = stage.now - WAKE;
    let width = stage.width.min(stage.height) * 0.46;
    let height = width * 1.18;
    let (x, y) = (stage.width / 2. - width / 2., stage.middle_y - height / 2.);
    let eye_w = width * 0.3;
    let eye_h = width * 0.2;

    // The clamps come down, the lids are forced, the head snaps.
    let grip = in_cubic(through(stage.now, WAKE + 1100, 380));
    let open = out_back(through(stage.now, WAKE + 1620, 620), 1.8);
    let awake = since >= 2240;
    // The head snapping back as the lids give: a hard jerk and three
    // smaller ones. Nought until the moment it starts, because the clock it
    // is read off has not begun.
    let tremble = match since.saturating_sub(1620) {
        at if at < 90 => 1.6 * (at as f32 / 90.),
        at if at < 210 => 1.6 - 3.2 * ((at - 90) as f32 / 120.),
        at if at < 320 => -1.6 + 2.3 * ((at - 210) as f32 / 110.),
        at if at < 480 => 0.7 - 0.7 * ((at - 320) as f32 / 160.),
        _ => 0.,
    };
    let quiver = if awake { sway(stage.now, 120, 0.5) } else { 0. };
    let scale = 1.1 - 0.1 * out_cubic(through(stage.now, WAKE, 900));
    let foot = (stage.width / 2., y + height);
    let cam = stage.cam.turned(foot, (tremble + quiver) / 360.).about(foot, 0., scale, scale);

    paint::glow(window, &stage.cam, stage.width / 2., stage.middle_y, width * 0.95, 0x4d79ff, 0.3 * alpha);
    paint::rect(window, &cam, (x, y, width, height), width * 0.45, shade(SKIN, alpha));
    paint::rect_edge(window, &cam, (x, y, width, height), width * 0.45, 2., shade(0xa9805a, alpha));

    let brow_y = y + height * 0.34 - height * 0.03 * open;
    for brow in [0.19, 0.53] {
        let thick = (eye_h * 0.16f32).max(3.);
        paint::rect(window, &cam, (x + width * brow, brow_y, eye_w * 0.9, thick), thick / 2., shade(0x4a3a2a, alpha));
    }

    let look = if awake { looking(since - 2240) } else { 0. };
    for eye in [0.18, 0.52] {
        forced_eye(window, &cam, (x + width * eye, y + height * 0.42 - eye_h / 2., eye_w, eye_h), open, grip, look, stage.height, alpha);
    }

    let nose_w = (width * 0.03f32).max(3.);
    paint::rect(window, &cam, (x + width / 2. - nose_w / 2., y + height * 0.5, nose_w, height * 0.14), nose_w / 2., shade(0xc9a37c, alpha));
    let mouth_w = width * (0.2 + open.min(1.) * 0.12);
    let mouth_h = (height * 0.055 * open.min(1.)).max(4.);
    paint::rect(
        window,
        &cam,
        (x + width / 2. - mouth_w / 2., y + height * 0.72, mouth_w, mouth_h),
        mouth_w.min(mouth_h) / 2.,
        shade(0x7a3b3b, alpha),
    );

    // Still asleep, until the lids come up.
    let asleep = (1. - open * 3.).max(0.) * alpha;
    if asleep > 0. {
        let clock = (since % 2400) as f32 / 2400.;
        for snore in 0..3usize {
            let at = (clock + snore as f32 / 3.) % 1.;
            let size = width * (0.08 + snore as f32 * 0.03);
            paint::word(
                window,
                cx,
                &cam,
                "Z",
                (x + width / 2. + width * 0.32 + at * width * 0.14, y + height * 0.16 - at * height * 0.24),
                size,
                shade(INK, (at * std::f32::consts::PI).sin() * asleep),
                false,
            );
        }
    }
}

/// Where it is looking, once it is looking at all: about, then back, then
/// about again, on a clock of its own.
fn looking(since: u128) -> f32 {
    let round = since % 3240;
    match round {
        at if at < 240 => out_cubic(at as f32 / 240.),
        at if at < 860 => 1.,
        at if at < 1160 => 1. - 2. * out_cubic((at - 860) as f32 / 300.),
        at if at < 1640 => -1.,
        at if at < 1900 => -1. + 1.35 * out_cubic((at - 1640) as f32 / 260.),
        at if at < 2600 => 0.35,
        at if at < 2840 => 0.35 - 0.35 * out_cubic((at - 2600) as f32 / 240.),
        _ => 0.,
    }
}

/// One eye, and the rod that is holding it open.
#[allow(clippy::too_many_arguments)]
fn forced_eye(window: &mut Window, cam: &Cam, (x, y, w, h): (f32, f32, f32, f32), open: f32, grip: f32, look: f32, screen: f32, alpha: f32) {
    let slit = (h * open.min(1.)).max(h * 0.05);
    let mid = (x + w / 2., y + h / 2.);

    paint::rect(window, cam, (x, mid.1 - slit / 2., w, slit), slit / 2., shade(CLOTH, alpha));
    let iris = h * 0.33;
    // The eye behind the lids is whole; only as much of it as the lids allow
    // is drawn, which is what the QML clipped to.
    let showing = (slit / h).min(1.);
    paint::disc(window, cam, mid.0 + look * h * 0.16, mid.1, iris, showing, shade(IRIS, alpha));
    paint::disc(window, cam, mid.0 + look * h * 0.16, mid.1, iris * (0.32 + open * 0.3), showing, shade(PUPIL, alpha));
    paint::rect_edge(window, cam, (x, mid.1 - slit / 2., w, slit), slit / 2., (h * 0.07f32).max(2.), shade(0x8a5f3c, alpha));

    // Two jaws on a rod, down from somewhere above the screen.
    let claw_h = (h * 0.14f32).max(4.);
    let down = (1. - grip) * screen;
    let shaft_w = (h * 0.1f32).max(3.);
    let claw_w = w * 0.92;
    let top = mid.1 - slit / 2. - claw_h - down;
    paint::rect(window, cam, (mid.0 - shaft_w / 2., top - screen, shaft_w, screen), 0., shade(0x9aa6bb, alpha));
    for at in [top, mid.1 + slit / 2. - down] {
        paint::rect(
            window,
            cam,
            (mid.0 - claw_w / 2., at, claw_w, claw_h),
            claw_h / 2.,
            paint::down(shade(0xf2f6ff, alpha), shade(0x7f8ca3, alpha)),
        );
    }
}

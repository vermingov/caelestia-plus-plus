//! The five acts, laid out as marks, back to front.
//!
//! One: a star drawn in light, a stroke at a time, that bursts and hangs
//! there. Two: the flag on its pole, waving. Three: night over a city, and
//! fireworks. Four: the portrait rising under a rain of eyes. Five: a face
//! asleep, and what wakes it.
//!
//! Everything is a reading off the clock: where a thing is, how much of it
//! shows, and how it is turned, are worked out afresh for every frame from
//! the time alone.

use std::f32::consts::{PI, TAU};

use super::marks::{Cam, Colour, Eye, Fill, Marks, Stroke, faded, hex};
use super::{FACE, FLAG, SKY, Stage, WAKE, in_cubic, in_out_quad, in_out_sine, out_back, out_cubic, through};

// The colours, once, by what they are rather than what they are made of.
const NIGHT: Colour = hex(0x040a1f, 1.);
const INK: Colour = hex(0xf4f7ff, 1.);
const BLUE: Colour = hex(0x3d6bff, 1.);
const PALE_BLUE: Colour = hex(0x9db6ff, 1.);
const HAZE: Colour = hex(0x4d79ff, 1.);
const FLAG_BLUE: Colour = hex(0x0038b8, 1.);
const CLOTH: Colour = hex(0xf6f8fb, 1.);
const GOLD: Colour = hex(0xf2c95c, 1.);
const IRIS: Colour = hex(0x2e6bd6, 1.);
const SKIN: Colour = hex(0xe6bf9b, 1.);
const BRIGHT_STEEL: Colour = hex(0xf2f6ff, 1.);
const DULL_STEEL: Colour = hex(0x6f7c93, 1.);

/// The fireworks: when each bursts after the night begins, where, as shares
/// of the screen, in what, and how big. The last is the finale.
const BURSTS: [(u128, (f32, f32), Colour, f32); 6] = [
    (560, (0.30, 0.30), BLUE, 1.0),
    (900, (0.69, 0.24), INK, 1.1),
    (1260, (0.50, 0.38), GOLD, 0.9),
    (1620, (0.21, 0.21), INK, 0.95),
    (1960, (0.79, 0.33), BLUE, 1.0),
    (2280, (0.52, 0.18), PALE_BLUE, 1.35),
];

/// When, after the waking begins, the pupils start to turn into stars:
/// once the eyes have been open long enough to be looking about.
const STARS: u128 = 2450;

/// How long a rocket climbs before it bursts, and how long a burst lasts.
const CLIMB: u128 = 520;
const BURNS: f32 = 1.8;

/// Everything that shakes the picture, and how hard: each burst, by its
/// size, and the clamps landing, which is the one that is meant to be felt.
pub fn jolts() -> Vec<(u128, f32)> {
    let mut all: Vec<(u128, f32)> = BURSTS.iter().map(|&(at, _, _, size)| (SKY + at, 1.5 + size * 1.5)).collect();
    all.push((WAKE + 1480, 9.));
    all
}

pub fn lay(stage: &Stage, marks: &mut Marks) {
    let dim = stage.reveal * stage.fading;
    // The dark over the desktop, which the whole picture sits in. Still: a
    // wash that moved with the camera would show its edges.
    marks.rect(&Cam::STILL, (0., 0., stage.width, stage.height), Fill::flat(NIGHT), 0.66 * dim);
    night(stage, marks);
    rays(stage, marks);
    motes(stage, marks);
    flag(stage, marks);
    star(stage, marks);
    fireworks(stage, marks);
    eye_rain(stage, marks);
    portrait(stage, marks);
    awakening(stage, marks);
    marks.lens(stage.screen(), (0.55 * dim, 0.085 * dim, stage.reveal, stage.flash()), (stage.now / 33) as f32, stage.fading);
}

/// How much of an act shows: up over `rise` from its start, down over `fall`
/// from its end.
fn showing(now: u128, (from, to): (u128, u128), rise: u128, fall: u128) -> f32 {
    if now < from || now >= to + fall {
        return 0.;
    }
    let up = if rise == 0 { 1. } else { through(now, from, rise) };
    up.min(1. - through(now, to, fall))
}

/// A value that swings between `-amp` and `+amp` for ever, one way in
/// `half`, the other in `half`.
fn sway(now: u128, half: u128, amp: f32) -> f32 {
    let round = now % (half * 2);
    let at = if round < half { in_out_sine(round as f32 / half as f32) } else { 1. - in_out_sine((round - half) as f32 / half as f32) };
    (at * 2. - 1.) * amp
}

/// A number between nought and one that is always the same for the same
/// thing, so that nothing about the crowds need be kept between frames.
fn wander(index: usize, salt: u32) -> f32 {
    let mut state = (index as u32).wrapping_mul(2_654_435_761).wrapping_add(salt.wrapping_mul(97_531)) | 1;
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    (state % 10_000) as f32 / 10_000.
}

fn between((ax, ay): (f32, f32), (bx, by): (f32, f32), at: f32) -> (f32, f32) {
    (ax + (bx - ax) * at, ay + (by - ay) * at)
}

fn blend(a: Colour, b: Colour, at: f32) -> Colour {
    std::array::from_fn(|channel| a[channel] + (b[channel] - a[channel]) * at)
}

/// How far round a clock of `period` it is, nought to one.
fn round(now: u128, period: u128) -> f32 {
    (now % period) as f32 / period as f32
}

// -- Behind everything -----------------------------------------------------

/// Motes drifting up, all the way through, in three planes: far ones small,
/// slow and nearly sharp; the middle ones in focus; near ones large, quick,
/// and blurred into discs of light the way a lens renders what is too close
/// to it.
fn motes(stage: &Stage, marks: &mut Marks) {
    // (how many, how long one takes to rise, size, strength, wander, blur)
    const PLANES: [(usize, u128, f32, f32, f32, f32); 3] =
        [(40, 12_000, 0.6, 0.32, 5., 0.3), (26, 7_600, 1.0, 0.58, 14., 0.05), (12, 4_800, 3.4, 0.32, 26., 0.9)];
    // The night has stars of its own.
    let quiet = 1. - 0.65 * showing(stage.now, (SKY, FACE), 300, 150);
    let dim = stage.reveal * stage.fading * quiet;
    for (plane, &(count, period, size, strength, wobble, blur)) in PLANES.iter().enumerate() {
        let clock = round(stage.now, period);
        let salt = plane as u32 * 977;
        for index in 0..count {
            let lane = wander(index + plane * 71, 1 + salt);
            let drift = wander(index + plane * 71, 2 + salt);
            let at = (clock + drift) % 1.;
            let x = lane * stage.width + (at * 12.6 + drift * 9.).sin() * wobble * stage.unit;
            let y = stage.height * (1.1 - at * 1.2);
            let radius = (1.4 + (index % 3) as f32 * 0.9) * size * stage.unit;
            let colour = if index % 4 == 0 { hex(0x7d9bff, 1.) } else { INK };
            let alpha = (at * PI).sin() * 0.9 * strength * dim;
            marks.mote(&stage.cam, (x, y), radius, faded(colour, alpha), blur);
        }
    }
}

/// Two fans of light turning against each other behind the star, the flag,
/// the face.
fn rays(stage: &Stage, marks: &mut Marks) {
    let now = stage.now;
    let hidden = showing(now, (SKY, FACE), 150, 150);
    let strength = stage.reveal * stage.fading * (1. - hidden) * if now >= FACE { 0.2 } else { 0.14 };
    if strength <= 0.001 {
        return;
    }
    let y = if now >= FACE { stage.middle_y + (stage.height * 0.42 - stage.middle_y) * in_out_quad(through(now, FACE, 800)) } else { stage.middle_y };
    let centre = stage.cam.point((stage.width / 2., y));
    let reach = stage.width.min(stage.height) * 0.8 * stage.cam.scale();
    let light = hex(0xdce6ff, 1.);
    marks.rays(stage.screen(), centre, (8., round(now, 40_000), reach), faded(light, strength));
    marks.rays(stage.screen(), centre, (6., -round(now, 61_000), reach * 0.75), faded(light, strength * 0.6));
}

// -- Act one: the star -----------------------------------------------------

/// The star's two triangles, each three corners, for a star `width` across
/// centred on `centre`.
fn hexagram((cx, cy): (f32, f32), width: f32) -> [[(f32, f32); 3]; 2] {
    let radius = width / 2. / 1.2;
    let corner = |turn: f32| (cx + radius * (turn * TAU).cos(), cy + radius * (turn * TAU).sin());
    [std::array::from_fn(|at| corner(-0.25 + at as f32 / 3.)), std::array::from_fn(|at| corner(0.25 + at as f32 / 3.))]
}

fn star(stage: &Stage, marks: &mut Marks) {
    let now = stage.now;
    let alpha = stage.reveal * stage.fading * showing(now, (0, FLAG), 0, 120);
    if alpha <= 0.001 {
        return;
    }
    let width = stage.width.min(stage.height) * 0.5;
    let centre = (stage.width / 2., stage.middle_y);
    marks.glow(&stage.cam, centre, width * 1.15, faded(BLUE, 0.32 * alpha * (0.45 + stage.pulse * 0.35)), 2.2);

    let arrive = out_back(through(now, 0, 1500), 1.4);
    let scale = (0.62 + 0.38 * arrive) * (0.97 + stage.pulse * 0.04);
    let turn = ((1. - arrive) * -12. + sway(now, 2400, 2.5)) / 360.;
    let cam = stage.cam.about(centre, turn, scale, scale);

    // Drawn on, a side at a time, both triangles at once.
    let drawing = in_out_sine(through(now, 220, 1050));
    let hot = 0.5 + stage.pulse * 0.15;
    let core = Stroke { width: width * 0.01, glow: width * 0.03, strength: hot, solid: 0.7, ..Stroke::light(INK, 0., 0., 0.) };
    let haze = Stroke { width: width * 0.012, glow: width * 0.09, strength: 0.16 + stage.pulse * 0.08, ..Stroke::light(BLUE, 0., 0., 0.) };
    for corners in hexagram(centre, width) {
        for side in 0..3 {
            let share = (drawing * 3. - side as f32).clamp(0., 1.);
            if share <= 0. {
                continue;
            }
            let (from, to) = (corners[side], corners[(side + 1) % 3]);
            marks.stroke(&cam, from, to, Stroke { drawn: (0., share), ..haze }, alpha);
            marks.stroke(&cam, from, to, Stroke { drawn: (0., share), ..core }, alpha);
            // The point of light drawing it.
            if share < 1. {
                marks.glow(&cam, between(from, to, share), width * 0.08, faded(INK, 0.8 * alpha), 3.5);
            }
        }
    }

    // Whole: a burst of light, a ring thrown out, and a streak across the
    // lens, the way a bright light flares in an anamorphic one.
    const WHOLE: u128 = 1270;
    if now >= WHOLE {
        let burst = out_cubic(through(now, WHOLE, 1500));
        let flare = 1. - through(now, WHOLE, 1400);
        marks.glow(&stage.cam, centre, width * (0.8 + burst * 0.6), faded(INK, flare * 0.45 * alpha), 3.);
        let ring = width * (0.55 + burst * 1.6);
        let edge = (4. - 3. * burst).max(1.2) * stage.unit;
        marks.oval(&stage.cam, centre, (ring, ring), Fill::flat(faded(INK, 0.)).edged(faded(INK, (1. - burst) * 0.7), edge), alpha);
        let across = stage.cam.point(centre).1;
        let streak = Stroke::light(PALE_BLUE, 1.2 * stage.unit, 16. * stage.unit, 1.);
        marks.stroke(&Cam::STILL, (0., across), (stage.width, across), streak, flare * 0.8 * alpha);
    }
}

// -- Act two: the flag -----------------------------------------------------

fn flag(stage: &Stage, marks: &mut Marks) {
    let now = stage.now;
    // Up twice: its own act, then small and high over the portrait.
    let alpha = stage.fading * (showing(now, (FLAG, SKY), 350, 120) + showing(now, (FACE, WAKE), 500, 400));
    if alpha <= 0.001 {
        return;
    }
    let unit = stage.unit;
    let width = stage.width * 0.46;
    let height = width * 8. / 11.;
    // Off the left edge until its act, then swept in past the mark and back.
    let from = -width * 1.6 - width / 2.;
    let to = stage.width / 2. - width / 2.;
    let x = from + (to - from) * out_back(through(now, FLAG, 900), 1.1);
    let lifted = in_out_quad(through(now, FACE, 800));
    let middle_y = stage.middle_y + (stage.height * 0.25 - stage.middle_y) * lifted;
    let scale = 1. - 0.58 * lifted;
    let y = middle_y - height / 2.;
    let hoist = (x, y + height / 2.);
    let cam = stage.cam.about((x + width / 2., middle_y), 0., scale, scale).turned(hoist, sway(now, 1400, 2.5) / 360.);

    // Light behind it, and its shadow thrown down and away.
    marks.glow(&cam, (x + width / 2., y + height / 2.), width * 0.8, faded(hex(0xbcd0ff, 1.), 0.25 * alpha), 2.);
    marks.rect(&cam, (x + width * 0.03, y + height * 0.09, width, height), Fill::flat(hex(0x000000, 0.45)).rounded(8. * unit).blurred(24. * unit), alpha);

    // The pole, its shine, and the gilded knob on top.
    let pole_w = (width * 0.016).max(4. * unit);
    let pole_x = x - pole_w - width * 0.012;
    let pole = (pole_x, y - height * 0.14, pole_w, height * 1.4);
    marks.rect(&cam, pole, Fill::down(hex(0xf1f4f9, 1.), hex(0x8a93a3, 1.)).rounded(pole_w / 2.), alpha);
    marks.rect(&cam, (pole_x + pole_w * 0.2, pole.1 + pole_w, pole_w * 0.25, pole.3 - pole_w * 2.), Fill::flat(hex(0xffffff, 0.55)).rounded(pole_w * 0.12), alpha);
    let knob = pole_w * 1.4;
    let knob_at = (pole_x + pole_w / 2., pole.1 - knob * 0.5);
    marks.oval(&cam, knob_at, (knob, knob), Fill::down(hex(0xfbe9a6, 1.), hex(0xa8822a, 1.)), alpha);
    marks.glow(&cam, (knob_at.0 - knob * 0.3, knob_at.1 - knob * 0.35), knob * 0.7, faded(INK, 0.7 * alpha), 5.);

    // The cloth, waving; and a band of light crossing it, over and over, in
    // its own act only.
    let phase = now as f32 / 1000. * 3.4;
    let sheen = match now {
        now if now < SKY && (now - FLAG) % 2800 < 1900 => (-0.4 + 1.9 * in_out_quad(((now - FLAG) % 2800) as f32 / 1900.), 0.3),
        _ => (0., 0.),
    };
    marks.cloth(&cam, (x, y, width, height), (CLOTH, FLAG_BLUE), (phase, 0.055), sheen, alpha);
}

// -- Act three: the night --------------------------------------------------

/// Where the light of the latest burst is on the screen, how far it reaches
/// and how bright it still is.
fn burst_light(stage: &Stage) -> [f32; 4] {
    let since_night = stage.now.saturating_sub(SKY);
    let latest = BURSTS.iter().rev().find(|(at, ..)| since_night >= *at);
    let Some(&(at, (fx, fy), _, size)) = latest else { return [0.; 4] };
    let since = (since_night - at) as f32 / 1000.;
    let at = stage.cam.point((stage.width * fx, stage.height * fy));
    let reach = stage.width.min(stage.height) * 0.7 * size * stage.cam.scale();
    [at.0, at.1, reach, (-since * 2.2).exp() * size]
}

fn night(stage: &Stage, marks: &mut Marks) {
    let presence = stage.fading * showing(stage.now, (SKY, FACE), 260, 120);
    if presence <= 0.001 {
        return;
    }
    let light = burst_light(stage);
    marks.sky(stage.screen(), (hex(0x01030d, 1.), hex(0x0f1842, 1.)), 0.22, light, presence);
    let (ground, tallest) = (stage.height, stage.height * 0.2);
    marks.skyline(stage.screen(), (hex(0x03060f, 1.), hex(0xffc978, 1.)), (ground, tallest), (light[0], light[3]), presence);
}

fn fireworks(stage: &Stage, marks: &mut Marks) {
    if stage.now < SKY {
        return;
    }
    let presence = stage.fading * showing(stage.now, (SKY, FACE), 0, 120);
    if presence <= 0.001 {
        return;
    }
    let since_night = stage.now - SKY;
    for (index, &(at, (fx, fy), colour, size)) in BURSTS.iter().enumerate() {
        let centre = (stage.width * fx, stage.height * fy);
        let launch = at.saturating_sub(CLIMB);
        if since_night < launch {
            continue;
        }
        if since_night < at {
            rocket(stage, marks, centre, through(since_night, launch, CLIMB), presence);
        } else {
            burst(stage, marks, index, centre, (since_night - at) as f32 / 1000., (colour, size), presence);
        }
    }
}

/// A rocket climbing to where it bursts, slowing as it goes, shedding
/// sparks.
fn rocket(stage: &Stage, marks: &mut Marks, centre: (f32, f32), progress: f32, presence: f32) {
    let unit = stage.unit;
    let from = (centre.0 - stage.width * 0.02, stage.height * 1.02);
    let climb = out_cubic(progress);
    let head = between(from, centre, climb);
    let behind = between(from, centre, (climb - 0.09).max(0.));
    let trail = Stroke { tail: faded(GOLD, 0.), head: INK, ..Stroke::light(INK, 1.6 * unit, 6. * unit, 0.9) };
    marks.stroke(&stage.cam, behind, head, trail, presence);
    marks.glow(&stage.cam, head, 11. * unit, faded(INK, 0.8 * presence), 3.);
    for spark in 0..5 {
        let back = climb - 0.05 - spark as f32 * 0.045;
        if back <= 0. {
            continue;
        }
        let at = between(from, centre, back);
        let fallen = (spark as f32 + 1.) * 5. * unit * progress;
        let across = (wander(spark, 5) - 0.5) * 8. * unit;
        marks.mote(&stage.cam, (at.0 + across, at.1 + fallen), 1.5 * unit, faded(GOLD, 0.7 * (1. - back / climb.max(0.01)) * presence), 0.15);
    }
}

/// One burst, `since` seconds after it went: a flash, and sparks thrown out
/// in every direction, dragged by the air, pulled down, fading — each drawn
/// as the streak it makes in the time a frame is open.
fn burst(stage: &Stage, marks: &mut Marks, index: usize, centre: (f32, f32), since: f32, (colour, size): (Colour, f32), presence: f32) {
    if since > BURNS {
        return;
    }
    let unit = stage.unit;
    let reach = stage.width.min(stage.height) * 0.24 * size;
    let finale = size > 1.2;
    let sparks = if finale { 120 } else { 84 };
    let fade = (1. - since / BURNS).powf(1.4);

    let flash = (1. - since / 0.35).max(0.);
    marks.glow(&stage.cam, centre, reach * 0.75, faded(blend(colour, INK, 0.4), flash * 0.5 * presence), 2.2);

    const DRAG: f32 = 2.4;
    let gravity = stage.height * 0.16;
    let spark_at = |direction: (f32, f32), speed: f32, t: f32| {
        let out = reach * speed * (1. - (-DRAG * t).exp());
        (centre.0 + direction.0 * out, centre.1 + direction.1 * out + 0.5 * gravity * t * t)
    };
    let salt = index as u32 * 13;
    for spark in 0..sparks {
        let angle = (spark as f32 / sparks as f32 + (wander(spark, salt + 7) - 0.5) * 0.02) * TAU;
        let direction = (angle.cos(), angle.sin());
        let speed = 0.78 + 0.35 * wander(spark, salt + 8);
        let head = spark_at(direction, speed, since);
        let tail = spark_at(direction, speed, (since - 0.08).max(0.));
        let twinkle = if finale { 0.55 + 0.45 * (since * 38. + spark as f32 * 1.7).sin() } else { 1. };
        let hot = blend(INK, colour, (since / 0.3).min(1.));
        let streak = Stroke { tail: faded(colour, 0.15), head: hot, ..Stroke::light(hot, 1.3 * unit * (1. - since / BURNS * 0.4), 6. * unit, 0.9) };
        marks.stroke(&stage.cam, tail, head, streak, fade * twinkle * presence);
        // Gold crackles as it dies.
        if colour == GOLD && since > 1.0 && wander(spark, salt + 9 + (since * 20.) as u32) > 0.7 {
            marks.glow(&stage.cam, head, 5. * unit, faded(INK, 0.8 * fade * presence), 4.);
        }
    }
}

// -- Act four: the face ----------------------------------------------------

fn portrait(stage: &Stage, marks: &mut Marks) {
    let now = stage.now;
    let alpha = stage.fading * showing(now, (FACE, WAKE), 600, 600);
    if alpha <= 0.001 {
        return;
    }
    let height = stage.height * 0.52;
    let width = height * 0.8;
    let rise = if now < WAKE { out_back(through(now, FACE, 900), 1.2) } else { 1. - out_back(through(now, WAKE, 900), 1.2) };
    let x = stage.width / 2. - width / 2.;
    let y = stage.height - height * rise;
    let foot = (stage.width / 2., y + height);
    let scale = 1. + stage.pulse * 0.03;
    let cam = stage.cam.turned(foot, sway(now, 1100, 2.5) / 360.).about(foot, 0., scale, scale);

    // A light behind, as a stage has, and the rim it catches.
    marks.glow(&stage.cam, (stage.width / 2., y + height * 0.45), height * 0.9, faded(HAZE, 0.45 * alpha), 1.8);
    marks.portrait(&cam, (x, y, width, height), faded(hex(0xa9c4ff, 1.), 0.45), (2., -2.), alpha);
}

/// Eyes falling past, every one of them watching the middle of the screen.
fn eye_rain(stage: &Stage, marks: &mut Marks) {
    let now = stage.now;
    let alpha = stage.fading * showing(now, (FACE, WAKE), 600, 600);
    if alpha <= 0.001 {
        return;
    }
    let clock = round(now - FACE, 3400);
    for index in 0..22usize {
        let lane = wander(index, 3);
        let drop = wander(index, 4);
        let at = (clock + drop) % 1.;
        // Some nearer than others: bigger, and fastest across the screen.
        let depth = 0.75 + (index % 3) as f32 * 0.25;
        let width = (34. + (index % 3) as f32 * 10.) * stage.unit * depth;
        let height = width * 0.58;
        let x = lane * stage.width + (at * 12.6 + drop * 6.3).sin() * 30. * stage.unit;
        let y = -height + (stage.height + height * 2.) * at;
        let shown = ((at * PI).sin() * 1.8).min(1.) * alpha;
        if shown <= 0. {
            continue;
        }
        let blink = ((at * 18.8 + drop * 40.).sin() - 0.86).max(0.) / 0.14;
        let mid = (x + width / 2., y + height / 2.);
        let cam = stage.cam.turned(mid, (at * 6.3 + drop * 6.3).sin() * 16. / 360.);
        let across = ((stage.width / 2. - mid.0) / (stage.width / 2.)).clamp(-1., 1.);
        let down = ((stage.height * 0.45 - mid.1) / (stage.height / 2.)).clamp(-1., 1.);
        marks.glow(&cam, mid, width * 0.85, faded(PALE_BLUE, 0.14 * shown), 2.);
        let eye = Eye { iris: IRIS, skin: SKIN, open: 1., look: (across, down), blink, star: 0., in_face: false };
        marks.eye(&cam, (x, y, width, height), eye, shown);
    }
}

// -- Act five: the waking --------------------------------------------------

fn awakening(stage: &Stage, marks: &mut Marks) {
    let now = stage.now;
    if now < WAKE {
        return;
    }
    let alpha = stage.reveal * stage.fading * through(now, WAKE, 600);
    if alpha <= 0.001 {
        return;
    }
    let unit = stage.unit;
    let since = now - WAKE;
    let width = stage.width.min(stage.height) * 0.46;
    let height = width * 1.18;
    let (x, y) = (stage.width / 2. - width / 2., stage.middle_y - height / 2.);
    let eye_w = width * 0.3;
    let eye_h = width * 0.2;

    // The clamps come down, the lids are forced, the head snaps.
    let grip = in_cubic(through(now, WAKE + 1100, 380));
    let open = out_back(through(now, WAKE + 1620, 620), 1.8);
    let awake = since >= 2240;
    // The head snapping back as the lids give: a hard jerk and three
    // smaller ones. Nought until the moment it starts.
    let tremble = match since.saturating_sub(1620) {
        at if since < 1620 || at == 0 => 0.,
        at if at < 90 => 1.6 * (at as f32 / 90.),
        at if at < 210 => 1.6 - 3.2 * ((at - 90) as f32 / 120.),
        at if at < 320 => -1.6 + 2.3 * ((at - 210) as f32 / 110.),
        at if at < 480 => 0.7 - 0.7 * ((at - 320) as f32 / 160.),
        _ => 0.,
    };
    let quiver = if awake { sway(now, 120, 0.5) } else { 0. };
    let scale = 1.1 - 0.1 * out_cubic(through(now, WAKE, 900));
    let foot = (stage.width / 2., y + height);
    let cam = stage.cam.turned(foot, (tremble + quiver) / 360.).about(foot, 0., scale, scale);

    marks.glow(&stage.cam, (stage.width / 2., stage.middle_y), width * 1.1, faded(HAZE, 0.35 * alpha), 2.);
    marks.oval(&cam, (stage.width / 2., y + height * 1.02), (width * 0.42, height * 0.07), Fill::flat(hex(0x000000, 0.45)).blurred(18. * unit), alpha);
    for (side, lit) in [(-1., 0xf0cba6), (1., 0xd9a67c)] {
        let ear = (stage.width / 2. + side * width * 0.49, y + height * 0.46);
        marks.oval(&cam, ear, (width * 0.075, height * 0.085), Fill::down(hex(lit, 1.), hex(0xc99268, 1.)).edged(hex(0xa9805a, 1.), 2. * unit), alpha);
    }
    // The head, lit from above, a shine on the brow and colour in the cheeks.
    let head = Fill::down(hex(0xf2d2b0, 1.), hex(0xd2a07a, 1.)).rounded(width * 0.45).edged(hex(0xa9805a, 1.), 2. * unit);
    marks.rect(&cam, (x, y, width, height), head, alpha);
    // Its form: turned away from the light down one side.
    marks.oval(&cam, (x + width * 0.3, y + height * 0.62), (width * 0.34, height * 0.42), Fill::flat(hex(0x6b3d24, 0.2)).blurred(width * 0.12), alpha);
    marks.glow(&cam, (x + width * 0.4, y + height * 0.2), width * 0.32, faded(INK, 0.14 * alpha), 2.5);
    for cheek in [0.22, 0.78] {
        marks.glow(&cam, (x + width * cheek, y + height * 0.63), width * 0.15, faded(hex(0xff6b6b, 1.), 0.16 * alpha), 2.);
    }

    let brow_y = y + height * 0.33 - height * 0.045 * open;
    for brow in [0.19, 0.53] {
        let thick = (eye_h * 0.17).max(3. * unit);
        marks.rect(&cam, (x + width * brow, brow_y, eye_w * 0.9, thick), Fill::down(hex(0x5a4632, 1.), hex(0x3d2f22, 1.)).rounded(thick / 2.), alpha);
    }

    let look = if awake { looking(since - 2240) } else { 0. };
    // Awake, and then something else: the pupils turn into stars, and catch
    // the light as they finish.
    let star = in_out_sine(through(now, WAKE + STARS, 750));
    let gleam = if since >= STARS + 750 { 1. - through(now, WAKE + STARS + 750, 700) } else { 0. };
    for eye in [0.18, 0.52] {
        let socket = (x + width * eye, y + height * 0.42 - eye_h / 2., eye_w, eye_h);
        let seen = Eye { iris: IRIS, skin: SKIN, open: open.max(0.04), look: (look, 0.), blink: 0., star, in_face: true };
        marks.eye(&cam, socket, seen, alpha);
        if gleam > 0. {
            let centre = (socket.0 + eye_w / 2. + look * eye_w * 0.19, socket.1 + eye_h / 2.);
            marks.glow(&cam, centre, eye_w * 0.7, faded(PALE_BLUE, 0.55 * gleam * alpha), 3.);
        }
        clamps(marks, &cam, socket, (grip, open), stage.height, unit, alpha);
    }

    let nose_w = (width * 0.05).max(3. * unit);
    marks.oval(&cam, (x + width / 2., y + height * 0.66), (nose_w * 1.4, nose_w * 0.5), Fill::flat(hex(0x7a4a2b, 0.35)).blurred(3. * unit), alpha);
    marks.rect(&cam, (x + width / 2. - nose_w / 2., y + height * 0.5, nose_w, height * 0.16), Fill::down(hex(0xecc39e, 1.), hex(0xc7976d, 1.)).rounded(nose_w / 2.), alpha);
    let mouth_w = width * (0.2 + open.min(1.) * 0.12);
    let mouth_h = (height * 0.06 * open.min(1.)).max(4. * unit);
    let mouth = Fill::down(hex(0x5a1e22, 1.), hex(0x2e0c0f, 1.)).rounded(mouth_w.min(mouth_h) / 2.).edged(hex(0x9a5048, 1.), 1.5 * unit);
    marks.rect(&cam, (x + width / 2. - mouth_w / 2., y + height * 0.72, mouth_w, mouth_h), mouth, alpha);

    // Still asleep, until the lids come up.
    let asleep = (1. - open * 3.).max(0.) * alpha;
    if asleep > 0. {
        let clock = round(since, 2400);
        for snore in 0..3usize {
            let at = (clock + snore as f32 / 3.) % 1.;
            let size = width * (0.08 + snore as f32 * 0.03);
            let place = (x + width * 0.7 + at * width * 0.14, y + height * 0.14 - at * height * 0.24);
            letter_z(marks, &cam, place, size, (at * PI).sin() * asleep);
        }
    }
}

/// A Z, in strokes of light.
fn letter_z(marks: &mut Marks, cam: &Cam, (x, y): (f32, f32), size: f32, alpha: f32) {
    let stroke = Stroke { solid: 0.6, ..Stroke::light(INK, size * 0.07, size * 0.3, 0.55) };
    let (left, right, top, bottom) = (x, x + size * 0.8, y, y + size);
    marks.stroke(cam, (left, top), (right, top), stroke, alpha);
    marks.stroke(cam, (right, top), (left, bottom), stroke, alpha);
    marks.stroke(cam, (left, bottom), (right, bottom), stroke, alpha);
}

/// Where it is looking, once it is looking at all: about, then back, then
/// about again, on a clock of its own.
fn looking(since: u128) -> f32 {
    match since % 3240 {
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

/// The rod holding one eye open, and its two jaws, down from somewhere above
/// the screen: `grip` of the way down, the lids `open` between them.
fn clamps(marks: &mut Marks, cam: &Cam, (x, y, w, h): (f32, f32, f32, f32), (grip, open): (f32, f32), screen: f32, unit: f32, alpha: f32) {
    let slit = (h * open.min(1.)).max(h * 0.05);
    let mid = (x + w / 2., y + h / 2.);
    let claw_h = (h * 0.16).max(4. * unit);
    let down = (1. - grip) * screen;
    let shaft_w = (h * 0.11).max(3. * unit);
    let claw_w = w * 0.95;
    let top = mid.1 - slit / 2. - claw_h - down;
    let shaft = (mid.0 - shaft_w / 2., top - screen, shaft_w, screen + claw_h * 0.5);
    marks.rect(cam, shaft, Fill::flat(DULL_STEEL).rounded(shaft_w * 0.3), alpha);
    marks.rect(cam, (shaft.0 + shaft_w * 0.18, shaft.1, shaft_w * 0.22, shaft.3), Fill::flat(faded(BRIGHT_STEEL, 0.7)), alpha);
    for at in [top, mid.1 + slit / 2. - down] {
        let jaw = Fill::down(BRIGHT_STEEL, DULL_STEEL).rounded(claw_h / 2.).edged(hex(0x2f3644, 0.6), 1. * unit);
        marks.rect(cam, (mid.0 - claw_w / 2., at, claw_w, claw_h), jaw, alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(now: u128) -> Vec<super::super::marks::Mark> {
        let stage = Stage::at(now, (1920., 1080.), 1.);
        let mut marks = Marks::with_room();
        lay(&stage, &mut marks);
        marks.list
    }

    #[test]
    fn every_act_has_something_in_it_and_no_frame_overflows() {
        for now in (0..super::super::OVER).step_by(97) {
            let marks = frame(now);
            assert!(marks.len() < super::super::marks::MOST, "{} marks at {now} ms", marks.len());
        }
        // The busiest moment: the finale bursting over the city.
        let busiest = frame(SKY + 2400).len();
        assert!(busiest > 300, "the finale has its sparks: {busiest}");
        for (act, at) in [("star", 2000), ("flag", FLAG + 1500), ("night", SKY + 1300), ("face", FACE + 1500), ("waking", WAKE + 3000)] {
            assert!(frame(at).len() > 10, "the {act} is empty");
        }
    }

    #[test]
    fn every_burst_has_climbed_before_it_goes_and_is_seen_before_the_cut() {
        for (at, ..) in BURSTS {
            assert!(at >= CLIMB, "a rocket would have to leave before the night began");
            // The last is still burning at the cut, which the flash covers;
            // each has had a good look before it.
            assert!(SKY + at + 300 < FACE, "a burst that goes at {at} is cut before it is seen");
        }
    }

    #[test]
    fn the_pupils_are_stars_well_before_the_picture_goes() {
        // Whole, and held for a second and a half before the fade begins.
        assert!(WAKE + STARS + 750 + 1500 < super::super::TRACK, "the stars have a moment before the fade");
        assert!(STARS > 2240, "not before the eyes are open and looking");
    }

    #[test]
    fn the_star_is_drawn_whole_by_the_time_it_bursts() {
        let drawing = in_out_sine(through(1270, 220, 1050));
        assert!((drawing - 1.).abs() < 0.001);
    }
}

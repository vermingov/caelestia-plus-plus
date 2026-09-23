//! The helix as a model: two backbones of beads, the base pairs between
//! them, and dust in the air around it, seen through a lens.
//!
//! Worked out here, on the processor, a frame at a time: a few hundred
//! shapes, each a place on the screen, a size and a colour, sorted from the
//! farthest to the nearest so that each is drawn over what is behind it. The
//! card is left with the only per-pixel work there is, shading each shape
//! where it lies — nothing is worked out for a pixel nothing covers.
//!
//! The shape of it is B-DNA's, in the helix's own radius: ten and a half
//! base pairs a turn, a rise of a third of the radius between them, and the
//! two backbones not opposite one another but a groove's width apart, which
//! is what gives the real molecule its wide and narrow grooves.

use std::f32::consts::TAU;

/// The three colours it is drawn in, as red, green and blue from nought to
/// one: the scheme's accent, a deep shade of it and a hot one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colours {
    pub deep: [f32; 3],
    pub primary: [f32; 3],
    pub hot: [f32; 3],
}

/// One shape, as the card reads it: 64 bytes, four vectors.
///
/// A ball has both ends in one place; a rod runs from one to the other.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Shape {
    /// One end: x and y in pixels, the radius there and how blurred it is.
    pub from: [f32; 4],
    /// The other end, the same.
    pub to: [f32; 4],
    /// The colour at `from`, and what kind of shape this is.
    pub first: [f32; 4],
    /// The colour at `to`, and how strongly the shape is drawn.
    pub second: [f32; 4],
}

/// What the shaders do with a shape; the fourth component of `first`.
pub mod kind {
    /// A lit sphere with a glow about it.
    pub const BALL: f32 = 0.;
    /// A lit rod in two colours, one from each end, parted in the middle.
    pub const PAIR: f32 = 1.;
    /// A lit rod in one colour.
    pub const LINK: f32 = 2.;
    /// A soft disc of light, unlit.
    pub const MOTE: f32 = 3.;
}

/// As many shapes as there can ever be in a frame: what is sent to the card
/// is room for this many, whatever a frame has.
pub const MOST: usize = 1024;

// The molecule, in units of its own radius.
const RISE: f32 = 0.34;
const TWIST: f32 = TAU / 10.5;
/// How far round the second backbone is from the first across a pair: less
/// than half a turn, so the grooves either side are a narrow and a wide one.
const GROOVE: f32 = 0.42 * TAU;
const BALL: f32 = 0.155;
const LINK: f32 = 0.1;
const PAIR: f32 = 0.07;
/// Enough pairs either side of the middle to run off both ends of any
/// screen at any angle; what does not reach the screen is dropped.
const REACH: i32 = 64;
/// One turn every so many seconds. Slow: a desktop is looked past, not at.
const TURN_SECONDS: f64 = 30.;

// The lens.
const FIELD_OF_VIEW: f32 = 0.52;
/// How far the middle of the helix is from the eye, in radii.
const DISTANCE: f32 = 15.;
/// Nothing nearer than this is drawn.
const NEAREST: f32 = 1.5;
/// How blurred a shape is for each radius it lies in front of the middle,
/// and behind it, in pixels, and the most it can be. More behind than in
/// front: the far end is what a camera's eye would lose first, and it is
/// where the helix, seen at a slant, is thickest with parts.
const BLUR_IN_FRONT: f32 = 9.;
const BLUR_BEHIND: f32 = 16.;
const MOST_BLUR: f32 = 64.;

// Where it lies: the axis leans up to the right, and its right-hand end is
// turned away into the distance.
const LEAN: f32 = -0.33;
const YAW: f32 = -0.38;
/// Where the middle of the helix is, as a share of the screen from its
/// centre: a little right of it and a little high.
const SHIFT: (f32, f32) = (0.04, -0.03);

// The air.
const MOTES: u32 = 34;
const MOTE_NEAR: f32 = 4.;
const MOTE_FAR: f32 = 30.;

/// Colour lost to the dark with distance: from here, up to this much.
const FOG_FROM: f32 = 15.5;
const FOG_TO: f32 = 21.;
const FOG_MOST: f32 = 0.9;

/// What the background is told about the helix: where its axis crosses the
/// screen, so that the light it throws lies along it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Axis {
    pub origin: [f32; 2],
    pub direction: [f32; 2],
}

/// The frame being built, kept between frames so that none of it is
/// allocated again.
#[derive(Default)]
pub struct Helix {
    shapes: Vec<Shape>,
    order: Vec<(f32, u32)>,
}

impl Helix {
    /// The helix as it is `time` seconds into its turning, on a screen of
    /// `size` pixels: its shapes from the farthest to the nearest, and its
    /// axis.
    pub fn frame(&mut self, size: (u32, u32), time: f64, colours: &Colours) -> (impl ExactSizeIterator<Item = &Shape>, Axis) {
        let lens = Lens::new(size);
        let spin = ((time / TURN_SECONDS).fract() * f64::from(TAU)) as f32;
        let palette = Palette::new(colours);

        self.shapes.clear();
        self.order.clear();
        self.molecule(&lens, spin, &palette);
        self.dust(&lens, time, &palette);

        // Farthest first: each shape is laid over whatever is behind it.
        // They were made in nearly that order already, which a stable sort
        // finds and keeps rather than sorting again; depths are all
        // positive, and a positive float's bits sort as the float does.
        self.order.sort_by_key(|(depth, _)| std::cmp::Reverse(depth.to_bits()));
        self.order.truncate(MOST);
        let shapes = &self.shapes;
        (self.order.iter().map(move |(_, at)| &shapes[*at as usize]), lens.axis())
    }

    fn keep(&mut self, depth: f32, shape: Shape, lens: &Lens) {
        if lens.shows(&shape) {
            self.order.push((depth, self.shapes.len() as u32));
            self.shapes.push(shape);
        }
    }

    fn molecule(&mut self, lens: &Lens, spin: f32, palette: &Palette) {
        let Some((nearest, farthest)) = lens.pairs_in_view() else { return };
        // Round the axis by one pair's twist at a time, rather than a sine
        // and a cosine for every bead: the same angles, a few multiplies
        // apiece. From the far end to the near one, which is nearly the
        // order they are drawn in, so the sort after has little to do.
        let (twist_cos, twist_sin) = (TWIST.cos(), -TWIST.sin());
        let (groove_cos, groove_sin) = (GROOVE.cos(), GROOVE.sin());
        let (mut sin, mut cos) = (farthest as f32 * TWIST + spin).sin_cos();
        let mut before: Option<([f32; 3], [f32; 3])> = None;
        for pair in (nearest..=farthest).rev() {
            let along = pair as f32 * RISE;
            let a = lens.place([along, cos, sin]);
            let b = lens.place([along, cos * groove_cos - sin * groove_sin, sin * groove_cos + cos * groove_sin]);
            (cos, sin) = (cos * twist_cos - sin * twist_sin, sin * twist_cos + cos * twist_sin);

            // The backbones, bead to bead, and the beads themselves.
            if let Some((was_a, was_b)) = before {
                for ends in [(was_a, a), (was_b, b)] {
                    self.rod(lens, palette, trimmed(ends, BALL), LINK, (palette.link, palette.link), kind::LINK);
                }
            }
            before = Some((a, b));
            for at in [a, b] {
                let Some(ball) = lens.ball(at, BALL, palette.fog(palette.backbone, at[2]), kind::BALL, 1.) else { continue };
                self.keep(at[2], ball, lens);
            }

            // The pair between them, in the two colours of its bases, the
            // way round each of them happens to be on its strand.
            self.rod(lens, palette, trimmed((a, b), BALL), PAIR, palette.bases(pair), kind::PAIR);
        }
    }

    /// A rod between two points of the eye's space, in a colour from each end.
    fn rod(&mut self, lens: &Lens, palette: &Palette, (from, to): Ends, radius: f32, colours: ([f32; 3], [f32; 3]), kind: f32) {
        let (Some(start), Some(end)) = (lens.point(from, radius), lens.point(to, radius)) else { return };
        let shape = Shape {
            from: start,
            to: end,
            first: with(palette.fog(colours.0, from[2]), kind),
            second: with(palette.fog(colours.1, to[2]), 1.),
        };
        self.keep((from[2] + to[2]) / 2., shape, lens);
    }

    /// Dust in the air, near and far, drifting along the helix. Nearly all
    /// of it out of focus, which is what makes it read as depth.
    fn dust(&mut self, lens: &Lens, time: f64, palette: &Palette) {
        for mote in 0..MOTES {
            let roll = |salt: u32| hash(mote * 7 + salt);
            let depth = MOTE_NEAR + (MOTE_FAR - MOTE_NEAR) * roll(1);
            // Spread over what the screen shows at that depth, and a little
            // past it, so that none comes in at an edge in plain sight.
            let (half_width, half_height) = lens.half_extent(depth);
            let span = (2.4 * half_width, 2.4 * half_height);
            let speed = 0.02 + 0.05 * roll(4);
            let drift = (time * f64::from(speed)) as f32;
            let x = wrap(roll(2) * span.0 + drift * span.0 * 0.6, span.0) - span.0 / 2.;
            let y = wrap(roll(3) * span.1 - drift * span.1 * 0.25, span.1) - span.1 / 2.;
            let radius = 0.04 + 0.08 * roll(5);
            let strength = 0.12 + 0.3 * roll(6);
            let colour = if roll(8) < 0.3 { palette.hot } else { palette.backbone };
            let at = [x, y, depth];
            let Some(shape) = lens.ball(at, radius, colour, kind::MOTE, strength) else { continue };
            self.keep(depth, shape, lens);
        }
    }
}

/// The colours of the parts, each out of the three it is given.
struct Palette {
    backbone: [f32; 3],
    link: [f32; 3],
    hot: [f32; 3],
    /// The two kinds of pair, each two bases.
    pairs: [([f32; 3], [f32; 3]); 2],
    /// What the far distance fades into: the dark of the background.
    dark: [f32; 3],
}

impl Palette {
    fn new(colours: &Colours) -> Palette {
        let Colours { deep, primary, hot } = *colours;
        Palette {
            backbone: primary,
            link: mix(primary, deep, 0.35),
            hot,
            pairs: [(hot, mix(deep, primary, 0.4)), (mix(primary, hot, 0.55), mix(primary, deep, 0.15))],
            dark: dark_of(colours),
        }
    }

    /// The bases of the `pair`th pair, strand by strand: which of the two
    /// kinds it is and which way round, as a sequence would have it.
    fn bases(&self, pair: i32) -> ([f32; 3], [f32; 3]) {
        let roll = hash(pair.rem_euclid(1 << 16) as u32 + 101);
        let (one, other) = self.pairs[usize::from(roll < 0.5)];
        if (roll * 4.).fract() < 0.5 { (one, other) } else { (other, one) }
    }

    /// The colour the distance leaves of `colour` at `depth`.
    fn fog(&self, colour: [f32; 3], depth: f32) -> [f32; 3] {
        let lost = ((depth - FOG_FROM) / (FOG_TO - FOG_FROM)).clamp(0., 1.) * FOG_MOST;
        mix(colour, self.dark, lost)
    }
}

/// The background's own dark, which the shader works out the same way: the
/// deep shade, nearly to black.
pub fn dark_of(colours: &Colours) -> [f32; 3] {
    colours.deep.map(|part| part * 0.07 + 0.012)
}

/// The eye: where it is, which way the helix lies before it, and how a point
/// in the helix's own space lands on the screen.
struct Lens {
    size: (f32, f32),
    focal: f32,
    /// The helix's axis and its other two directions, in the eye's space.
    turn: [[f32; 3]; 3],
    centre: [f32; 3],
}

impl Lens {
    fn new((width, height): (u32, u32)) -> Lens {
        let size = (width.max(1) as f32, height.max(1) as f32);
        let focal = size.1 / (2. * (FIELD_OF_VIEW / 2.).tan());
        let (sin_yaw, cos_yaw) = YAW.sin_cos();
        // On a screen taller than it is wide the helix stands up, or it would
        // cross the narrow way and leave most of the screen empty.
        let lean = if size.1 > size.0 { LEAN - std::f32::consts::FRAC_PI_2 } else { LEAN };
        let (sin_lean, cos_lean) = lean.sin_cos();
        // Turned away about the vertical, then leant in the plane of the
        // screen; the columns are where the helix's x, y and z go.
        let yawed = [[cos_yaw, 0., -sin_yaw], [0., 1., 0.], [sin_yaw, 0., cos_yaw]];
        let lean = |[x, y, z]: [f32; 3]| [x * cos_lean - y * sin_lean, x * sin_lean + y * cos_lean, z];
        let turn = [lean(yawed[0]), lean(yawed[1]), lean(yawed[2])];
        let centre = [
            SHIFT.0 * size.0 / focal * DISTANCE,
            SHIFT.1 * size.1 / focal * DISTANCE,
            DISTANCE,
        ];
        Lens { size, focal, turn, centre }
    }

    /// A point of the helix's own space, in the eye's.
    fn place(&self, [x, y, z]: [f32; 3]) -> [f32; 3] {
        let [a, b, c] = self.turn;
        [
            self.centre[0] + a[0] * x + b[0] * y + c[0] * z,
            self.centre[1] + a[1] * x + b[1] * y + c[1] * z,
            self.centre[2] + a[2] * x + b[2] * y + c[2] * z,
        ]
    }

    /// Where a point of the eye's space lands, how big `radius` is there,
    /// and how blurred: one end of a shape.
    fn point(&self, [x, y, z]: [f32; 3], radius: f32) -> Option<[f32; 4]> {
        if z < NEAREST {
            return None;
        }
        let scale = self.focal / z;
        let behind = z - DISTANCE;
        let blur = (if behind > 0. { behind * BLUR_BEHIND } else { -behind * BLUR_IN_FRONT }).min(MOST_BLUR);
        Some([self.size.0 / 2. + x * scale, self.size.1 / 2. + y * scale, radius * scale, blur])
    }

    fn ball(&self, at: [f32; 3], radius: f32, colour: [f32; 3], kind: f32, strength: f32) -> Option<Shape> {
        let point = self.point(at, radius)?;
        Some(Shape { from: point, to: point, first: with(colour, kind), second: with(colour, strength) })
    }

    /// Whether any of `shape`, its blur and its glow included, reaches the
    /// screen.
    fn shows(&self, shape: &Shape) -> bool {
        let reach = |end: [f32; 4]| 2.2 * end[2] + end[3];
        let margin = reach(shape.from).max(reach(shape.to));
        let low = (shape.from[0].min(shape.to[0]) - margin, shape.from[1].min(shape.to[1]) - margin);
        let high = (shape.from[0].max(shape.to[0]) + margin, shape.from[1].max(shape.to[1]) + margin);
        high.0 >= 0. && high.1 >= 0. && low.0 <= self.size.0 && low.1 <= self.size.1
    }

    /// The first and last pair any of which can reach the screen: those
    /// whose piece of axis lands within a helix's width of it, glow and blur
    /// included. The rest are not worked out at all.
    fn pairs_in_view(&self) -> Option<(i32, i32)> {
        let reaches = |pair: i32| {
            let [x, y, z] = self.place([pair as f32 * RISE, 0., 0.]);
            let Some([x, y, radius, blur]) = self.point([x, y, z], 1. + BALL * 2.3) else { return false };
            let margin = radius + blur;
            x > -margin && y > -margin && x < self.size.0 + margin && y < self.size.1 + margin
        };
        let nearest = (-REACH..=REACH).find(|pair| reaches(*pair))?;
        let farthest = (nearest..=REACH).rev().find(|pair| reaches(*pair))?;
        // One more at each end, for the backbone that runs on to it.
        Some(((nearest - 1).max(-REACH), (farthest + 1).min(REACH)))
    }

    /// Half the width and height of what the screen shows at `depth`.
    fn half_extent(&self, depth: f32) -> (f32, f32) {
        (self.size.0 / 2. * depth / self.focal, self.size.1 / 2. * depth / self.focal)
    }

    /// The axis where it crosses the middle of the helix, on the screen.
    fn axis(&self) -> Axis {
        let project = |at: [f32; 3]| {
            let scale = self.focal / at[2];
            [self.size.0 / 2. + at[0] * scale, self.size.1 / 2. + at[1] * scale]
        };
        let origin = project(self.place([0.; 3]));
        let along = project(self.place([1., 0., 0.]));
        let (dx, dy) = (along[0] - origin[0], along[1] - origin[1]);
        let length = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
        Axis { origin, direction: [dx / length, dy / length] }
    }
}

/// The two ends of a rod, in the eye's space.
type Ends = ([f32; 3], [f32; 3]);

/// Both ends brought in by `radius`: a rod between two beads starts and
/// stops at their surfaces, not at their centres, or the bead behind would
/// be drawn with the rod lying across its face.
fn trimmed((from, to): Ends, radius: f32) -> Ends {
    let span = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let length = (span[0] * span[0] + span[1] * span[1] + span[2] * span[2]).sqrt();
    if length <= 2. * radius {
        let middle = [(from[0] + to[0]) / 2., (from[1] + to[1]) / 2., (from[2] + to[2]) / 2.];
        return (middle, middle);
    }
    let cut = radius / length;
    let along = |t: f32| [from[0] + span[0] * t, from[1] + span[1] * t, from[2] + span[2] * t];
    (along(cut), along(1. - cut))
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn with([r, g, b]: [f32; 3], w: f32) -> [f32; 4] {
    [r, g, b, w]
}

fn wrap(value: f32, span: f32) -> f32 {
    value.rem_euclid(span)
}

/// A number from nought to one that looks random and is the same every time
/// for the same `seed`.
fn hash(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Colours = Colours { deep: [0.58, 0.04, 0.0], primary: [1., 0.33, 0.29], hot: [1., 0.74, 0.72] };

    fn frame(helix: &mut Helix, size: (u32, u32), time: f64) -> Vec<Shape> {
        helix.frame(size, time, &RED).0.copied().collect()
    }

    #[test]
    fn a_shape_is_sixty_four_bytes() {
        assert_eq!(std::mem::size_of::<Shape>(), 64);
    }

    #[test]
    fn the_helix_fills_the_screen_and_fits_the_buffer() {
        let mut helix = Helix::default();
        for size in [(1920, 1200), (2560, 1440), (1080, 1920), (3840, 2160)] {
            let shapes = frame(&mut helix, size, 12.5);
            assert!(shapes.len() > 150, "{size:?} drew only {} shapes", shapes.len());
            assert!(shapes.len() <= MOST);
            let balls = shapes.iter().filter(|shape| shape.first[3] == kind::BALL).count();
            assert!(balls > 40, "{size:?} has {balls} beads on it");
        }
    }

    /// Farther shapes are smaller and come first: what is drawn later is
    /// drawn over it, and the near end of the helix must be on top.
    #[test]
    fn shapes_come_from_the_far_end_to_the_near() {
        let mut helix = Helix::default();
        let shapes = frame(&mut helix, (1920, 1200), 3.);
        let beads: Vec<f32> = shapes.iter().filter(|shape| shape.first[3] == kind::BALL).map(|shape| shape.from[2]).collect();
        let (first, last) = (beads[..8].iter().sum::<f32>(), beads[beads.len() - 8..].iter().sum::<f32>());
        assert!(first < last, "the first beads drawn ({first}) are bigger than the last ({last})");
    }

    #[test]
    fn nothing_drawn_lies_wholly_off_the_screen() {
        let mut helix = Helix::default();
        let lens = Lens::new((1920, 1200));
        for shape in frame(&mut helix, (1920, 1200), 7.) {
            assert!(lens.shows(&shape), "{shape:?} is off the screen");
            assert!(shape.from.iter().chain(&shape.to).all(|number| number.is_finite()));
        }
    }

    /// The middle of the helix is in focus and its ends are not.
    #[test]
    fn the_middle_is_sharp_and_the_ends_are_soft() {
        let lens = Lens::new((1920, 1200));
        let middle = lens.point(lens.place([0., 1., 0.]), BALL).unwrap();
        let near_end = lens.point(lens.place([-14., 1., 0.]), BALL).unwrap();
        let far_end = lens.point(lens.place([14., 1., 0.]), BALL).unwrap();
        assert!(middle[3] < 2., "the middle is blurred by {} pixels", middle[3]);
        assert!(near_end[3] > 12. && far_end[3] > 6., "the ends are blurred by {} and {}", near_end[3], far_end[3]);
        assert!(near_end[2] > middle[2] && middle[2] > far_end[2], "the near end is not the bigger");
    }

    /// A whole turn later it is where it was: the turning never jumps,
    /// however long the desktop has been up.
    #[test]
    fn a_whole_turn_later_it_is_where_it_was() {
        let mut helix = Helix::default();
        let molecule = |shapes: Vec<Shape>| shapes.into_iter().filter(|shape| shape.first[3] != kind::MOTE).collect::<Vec<_>>();
        let now = molecule(frame(&mut helix, (1920, 1200), 5.));
        let later = molecule(frame(&mut helix, (1920, 1200), 5. + TURN_SECONDS * 1000.));
        assert_eq!(now.len(), later.len());
        let apart = now.iter().zip(&later).map(|(a, b)| (a.from[0] - b.from[0]).abs() + (a.from[1] - b.from[1]).abs()).fold(0f32, f32::max);
        assert!(apart < 0.05, "a thousand turns on, a shape has moved {apart} pixels");
    }

    #[test]
    fn a_rod_between_beads_starts_at_their_surfaces() {
        let (from, to) = trimmed(([0., 0., 0.], [2., 0., 0.]), 0.5);
        assert_eq!((from, to), ([0.5, 0., 0.], [1.5, 0., 0.]));
        // Beads that touch leave no rod to draw between them.
        let (from, to) = trimmed(([0., 0., 0.], [0.8, 0., 0.]), 0.5);
        assert_eq!(from, to);
    }

    #[test]
    fn the_sequence_uses_both_kinds_of_pair_both_ways_round() {
        let palette = Palette::new(&RED);
        let pairs: Vec<_> = (0..40).map(|pair| palette.bases(pair)).collect();
        let [(a, b), (c, d)] = palette.pairs;
        for wanted in [(a, b), (b, a), (c, d), (d, c)] {
            assert!(pairs.contains(&wanted), "{wanted:?} never comes up");
        }
    }
}

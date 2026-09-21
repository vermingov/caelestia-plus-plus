//! The two drawings the desktop egg is made of, as the shapes they were
//! drawn as.
//!
//! They were a pair of SVGs the QML shell handed to Qt. GPUI will draw an
//! SVG, but only in one colour, and only without turning it — and the whole
//! scene is one thing leaning into another. So the shapes are here instead,
//! in the same order and the same colours their files had, and the turn and
//! the size are done to the points on the way past.

use gpui::{Hsla, PathBuilder, PathStyle, Pixels, Point, StrokeOptions, Window, px, rgba};

use super::paint::Cam;
use lyon::tessellation::{LineCap, LineJoin};

/// One step of an outline, in the drawing's own units.
#[derive(Clone, Copy)]
pub enum Step {
    Start(f32, f32),
    /// Straight to a point, as the files' `l` was.
    Line(f32, f32),
    /// A quadratic: the control point, then where it ends. Both where they
    /// are in the drawing, never where they are from the last point.
    Curve(f32, f32, f32, f32),
    /// A cubic: two control points, then where it ends.
    Bend(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Clone, Copy)]
pub enum Shape {
    Round { x: f32, y: f32, radius: f32 },
    /// A round shape squashed on one axis, turned by `turn` full turns.
    Oval { x: f32, y: f32, across: f32, down: f32, turn: f32 },
    Line { from: (f32, f32), to: (f32, f32) },
    Outline(&'static [Step]),
}

/// A shape and how it is painted: the colour inside it, the colour round it,
/// and how much of either shows.
#[derive(Clone, Copy)]
pub struct Part {
    pub shape: Shape,
    pub fill: Option<u32>,
    pub stroke: Option<(u32, f32)>,
    pub alpha: f32,
}

const fn part(shape: Shape, fill: u32, stroke: u32, width: f32) -> Part {
    Part { shape, fill: Some(fill), stroke: Some((stroke, width)), alpha: 1. }
}

const fn tint(shape: Shape, fill: u32, alpha: f32) -> Part {
    Part { shape, fill: Some(fill), stroke: None, alpha }
}

const fn line(shape: Shape, stroke: u32, width: f32, alpha: f32) -> Part {
    Part { shape, fill: None, stroke: Some((stroke, width)), alpha }
}

/// A whole drawing, and the box it was drawn in.
pub struct Sticker {
    pub across: f32,
    pub down: f32,
    pub parts: &'static [Part],
}

const INK: u32 = 0x7a4a2b;
const VEIN: u32 = 0xcf8f83;
const SHINE: u32 = 0xffffff;

/// The one that rises first.
pub const RISER: Sticker = Sticker {
    across: 220.,
    down: 270.,
    parts: &[
        part(Shape::Round { x: 72., y: 216., radius: 46. }, 0xf2bf94, INK, 7.),
        part(Shape::Round { x: 148., y: 216., radius: 46. }, 0xf6c9a3, INK, 7.),
        part(
            Shape::Outline(&[
                Step::Start(76., 210.),
                Step::Curve(70., 110., 84., 58.),
                Step::Curve(92., 26., 110., 26.),
                Step::Curve(128., 26., 136., 58.),
                Step::Curve(150., 110., 144., 210.),
                Step::Curve(110., 232., 76., 210.),
                Step::Close,
            ]),
            0xf6c9a3,
            INK,
            7.,
        ),
        part(
            Shape::Outline(&[
                Step::Start(84., 62.),
                Step::Curve(92., 30., 110., 30.),
                Step::Curve(128., 30., 136., 62.),
                Step::Curve(140., 78., 137., 86.),
                Step::Curve(124., 100., 96., 100.),
                Step::Curve(86., 92., 84., 78.),
                Step::Close,
            ]),
            0xe8a179,
            INK,
            7.,
        ),
        line(Shape::Line { from: (110., 38.), to: (110., 52.) }, INK, 6., 1.),
        line(
            Shape::Outline(&[Step::Start(92., 120.), Step::Curve(102., 132., 94., 146.), Step::Curve(86., 160., 98., 172.)]),
            VEIN,
            5.,
            0.8,
        ),
        line(
            Shape::Outline(&[
                Step::Start(124., 105.),
                Step::Curve(114., 121., 123., 135.),
                Step::Curve(131., 148., 121., 163.),
                Step::Curve(113., 175., 123., 187.),
            ]),
            VEIN,
            5.,
            0.8,
        ),
        line(Shape::Outline(&[Step::Start(104., 165.), Step::Curve(116., 175., 112., 191.)]), VEIN, 5., 0.8),
        tint(Shape::Oval { x: 97., y: 52., across: 7., down: 12., turn: -14. / 360. }, SHINE, 0.45),
        tint(Shape::Oval { x: 55., y: 200., across: 10., down: 14., turn: -20. / 360. }, SHINE, 0.35),
    ],
};

/// The one that joins it.
pub const PARTNER: Sticker = Sticker {
    across: 240.,
    down: 200.,
    parts: &[
        part(
            Shape::Outline(&[
                Step::Start(120., 18.),
                Step::Bend(62., 18., 32., 78., 32., 122.),
                Step::Bend(32., 170., 76., 188., 120., 188.),
                Step::Bend(164., 188., 208., 170., 208., 122.),
                Step::Bend(208., 78., 178., 18., 120., 18.),
                Step::Close,
            ]),
            0xf6c9a3,
            INK,
            7.,
        ),
        part(
            Shape::Outline(&[
                Step::Start(120., 64.),
                Step::Bend(106., 96., 106., 138., 120., 166.),
                Step::Bend(134., 138., 134., 96., 120., 64.),
                Step::Close,
            ]),
            0xe88ba0,
            INK,
            7.,
        ),
        part(Shape::Round { x: 120., y: 58., radius: 9. }, 0xe88ba0, INK, 6.),
        tint(Shape::Oval { x: 70., y: 120., across: 12., down: 8., turn: 0. }, 0xf0a8a0, 0.6),
        tint(Shape::Oval { x: 170., y: 120., across: 12., down: 8., turn: 0. }, 0xf0a8a0, 0.6),
        tint(Shape::Oval { x: 66., y: 60., across: 9., down: 14., turn: 18. / 360. }, SHINE, 0.4),
    ],
};

/// Where a drawing is, how big, and which way up.
#[derive(Clone, Copy)]
pub struct Placed {
    /// Where the drawing's own origin lands on the screen.
    pub at: Point<Pixels>,
    /// What it is turned about, in the drawing's own units. Both stickers
    /// turn about the foot, as the QML ones did.
    pub about: (f32, f32),
    pub turn: f32,
    pub scale: f32,
    /// How much of it shows, over whatever each part asks for.
    pub alpha: f32,
}

/// How round a shape is made of four bends. The number every drawing program
/// uses for the same job.
const ROUNDNESS: f32 = 0.552_284_75;

/// The dark it sits in, under everything else it is made of.
///
/// Without one it is a drawing laid on the screen; with one it is standing on
/// the bottom of it. Nine ovals rather than a blur, each wider and fainter
/// than the last, which is the same trick `glow` uses and the only one on
/// offer without a real shadow to cast.
fn footing(sticker: &Sticker, placed: Placed, cam: &Cam, window: &mut Window) {
    const RINGS: usize = 9;
    let width = sticker.across * placed.scale * 0.42;
    let (x, y) = (f32::from(placed.at.x), f32::from(placed.at.y) + sticker.down * placed.scale * 0.5);
    for ring in (0..RINGS).rev() {
        let out = (ring + 1) as f32 / RINGS as f32;
        let alpha = 0.22 * (1. - out) * placed.alpha;
        crate::ui::eggs::paint::disc(window, cam, x, y, width * (0.55 + out * 0.9), 0.16,
            crate::ui::eggs::paint::shade(0x000000, alpha));
    }
}

pub fn paint(sticker: &Sticker, placed: Placed, cam: &Cam, window: &mut Window) {
    footing(sticker, placed, cam, window);
    let (angle, (about_x, about_y)) = (placed.turn * std::f32::consts::TAU, placed.about);
    let (sin, cos) = (angle.sin(), angle.cos());
    let put = move |x: f32, y: f32| {
        let (dx, dy) = ((x - about_x) * placed.scale, (y - about_y) * placed.scale);
        cam.put(f32::from(placed.at.x) + dx * cos - dy * sin, f32::from(placed.at.y) + dx * sin + dy * cos)
    };

    for piece in sticker.parts {
        if let Some(colour) = piece.fill {
            let mut path = PathBuilder::fill();
            trace(&mut path, piece.shape, &put);
            if let Ok(built) = path.build() {
                window.paint_path(built, crate::ui::eggs::paint::modelled(colour, piece.alpha * placed.alpha));
            }
        }
        if let Some((colour, width)) = piece.stroke {
            let width = px(cam.span(width * placed.scale));
            let round = StrokeOptions::default()
                .with_line_width(f32::from(width))
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round);
            let mut path = PathBuilder::stroke(width).with_style(PathStyle::Stroke(round));
            trace(&mut path, piece.shape, &put);
            if let Ok(built) = path.build() {
                window.paint_path(built, shade(colour, piece.alpha * placed.alpha));
            }
        }
    }
}

fn shade(colour: u32, alpha: f32) -> Hsla {
    rgba((colour << 8) | (alpha.clamp(0., 1.) * 255.) as u32).into()
}

fn trace(path: &mut PathBuilder, shape: Shape, put: &impl Fn(f32, f32) -> Point<Pixels>) {
    match shape {
        Shape::Round { x, y, radius } => oval(path, x, y, radius, radius, 0., put),
        Shape::Oval { x, y, across, down, turn } => oval(path, x, y, across, down, turn, put),
        Shape::Line { from, to } => {
            path.move_to(put(from.0, from.1));
            path.line_to(put(to.0, to.1));
        }
        Shape::Outline(steps) => {
            for step in steps {
                match *step {
                    Step::Start(x, y) => path.move_to(put(x, y)),
                    Step::Line(x, y) => path.line_to(put(x, y)),
                    Step::Curve(cx, cy, x, y) => path.curve_to(put(x, y), put(cx, cy)),
                    Step::Bend(ax, ay, bx, by, x, y) => path.cubic_bezier_to(put(x, y), put(ax, ay), put(bx, by)),
                    Step::Close => path.close(),
                }
            }
        }
    }
}

/// A round or squashed shape, as four bends. `turn` is in full turns, about
/// the shape's own middle. Also what the scene's droplets are.
pub fn oval(path: &mut PathBuilder, x: f32, y: f32, across: f32, down: f32, turn: f32, put: &impl Fn(f32, f32) -> Point<Pixels>) {
    let angle = turn * std::f32::consts::TAU;
    let (sin, cos) = (angle.sin(), angle.cos());
    let on = |dx: f32, dy: f32| put(x + dx * cos - dy * sin, y + dx * sin + dy * cos);
    let (rx, ry) = (across * ROUNDNESS, down * ROUNDNESS);

    path.move_to(on(0., -down));
    path.cubic_bezier_to(on(across, 0.), on(rx, -down), on(across, -ry));
    path.cubic_bezier_to(on(0., down), on(across, ry), on(rx, down));
    path.cubic_bezier_to(on(-across, 0.), on(-rx, down), on(-across, ry));
    path.cubic_bezier_to(on(0., -down), on(-across, -ry), on(-rx, -down));
    path.close();
}

// -- The portrait the cinema's fourth act rises ---------------------------

const SUIT: u32 = 0x1b2440;
const LAPEL: u32 = 0x141b31;
const CREASE: u32 = 0xd29a70;

/// A statesman, as the file drew him: a dark suit, a light-blue tie, silver
/// hair swept back off a high forehead. Twenty-seven shapes, in the order
/// the file laid them down.
pub const PORTRAIT: Sticker = Sticker {
    across: 240.,
    down: 300.,
    parts: &[
        // Shoulders, and a collar wide enough to sit a heavy head on.
        tint(
            Shape::Outline(&[
                Step::Start(14., 300.),
                Step::Line(32., 238.),
                Step::Curve(50., 212., 96., 204.),
                Step::Line(120., 198.),
                Step::Line(144., 204.),
                Step::Curve(190., 212., 208., 238.),
                Step::Line(226., 300.),
                Step::Close,
            ]),
            SUIT,
            1.,
        ),
        // The neck, thick, and in shadow under the jaw.
        tint(Shape::Outline(&[Step::Start(97., 176.), Step::Line(97., 210.), Step::Line(143., 210.), Step::Line(143., 176.), Step::Close]), 0xc99b72, 1.),
        tint(Shape::Outline(&[Step::Start(97., 176.), Step::Curve(120., 196., 143., 176.), Step::Line(143., 188.), Step::Curve(120., 204., 97., 188.), Step::Close]), 0xa87e5c, 1.),
        // Shirt, then the lapels over it, then the tie.
        tint(
            Shape::Outline(&[
                Step::Start(96., 204.),
                Step::Line(120., 250.),
                Step::Line(144., 204.),
                Step::Line(132., 198.),
                Step::Line(120., 206.),
                Step::Line(108., 198.),
                Step::Close,
            ]),
            0xf2f5f8,
            1.,
        ),
        tint(Shape::Outline(&[Step::Start(96., 204.), Step::Line(122., 256.), Step::Line(80., 232.), Step::Close]), LAPEL, 1.),
        tint(Shape::Outline(&[Step::Start(144., 204.), Step::Line(118., 256.), Step::Line(160., 232.), Step::Close]), LAPEL, 1.),
        tint(
            Shape::Outline(&[
                Step::Start(120., 208.),
                Step::Line(110., 222.),
                Step::Line(117., 278.),
                Step::Line(120., 286.),
                Step::Line(123., 278.),
                Step::Line(130., 222.),
                Step::Close,
            ]),
            0x2f5fa8,
            1.,
        ),
        tint(Shape::Outline(&[Step::Start(120., 208.), Step::Line(110., 222.), Step::Line(120., 231.), Step::Line(130., 222.), Step::Close]), 0x24497f, 1.),

        // Ears, low and close to a wide skull.
        tint(Shape::Oval { x: 54., y: 122., across: 12., down: 20., turn: 0. }, 0xdba97f, 1.),
        tint(Shape::Oval { x: 186., y: 122., across: 12., down: 20., turn: 0. }, 0xdba97f, 1.),

        // The head. Broad at the temples, square at the jaw, heavy at the
        // chin: the shape is most of the likeness, and the old one was an
        // egg, which is why nothing drawn on it helped.
        tint(
            Shape::Outline(&[
                Step::Start(56., 104.),
                Step::Curve(56., 46., 120., 40.),
                Step::Curve(184., 46., 184., 104.),
                Step::Line(183., 126.),
                Step::Curve(181., 150., 172., 160.),
                Step::Curve(164., 176., 146., 186.),
                Step::Curve(133., 194., 120., 194.),
                Step::Curve(107., 194., 94., 186.),
                Step::Curve(76., 176., 68., 160.),
                Step::Curve(59., 150., 57., 126.),
                Step::Close,
            ]),
            0xe7bc93,
            1.,
        ),
        // Jowls: the weight either side of the chin that a smooth jaw
        // cannot suggest.
        tint(Shape::Outline(&[Step::Start(70., 156.), Step::Curve(66., 176., 84., 184.), Step::Curve(76., 172., 78., 156.), Step::Close]), 0xdbaa80, 1.),
        tint(Shape::Outline(&[Step::Start(170., 156.), Step::Curve(174., 176., 156., 184.), Step::Curve(164., 172., 162., 156.), Step::Close]), 0xdbaa80, 1.),

        // The hair. White, parted low on his left, and gone from the
        // temples — the high square forehead with a peak in the middle is
        // the single most recognisable thing about the head.
        tint(
            Shape::Outline(&[
                Step::Start(54., 112.),
                Step::Curve(48., 74., 62., 56.),
                Step::Curve(84., 32., 122., 34.),
                Step::Curve(164., 36., 180., 60.),
                Step::Curve(190., 78., 186., 112.),
                Step::Curve(180., 96., 178., 84.),
                Step::Curve(174., 70., 160., 64.),
                // The receded corner on one side, and the sweep across.
                Step::Curve(146., 58., 132., 62.),
                Step::Curve(120., 66., 108., 64.),
                Step::Curve(92., 62., 80., 74.),
                Step::Curve(68., 86., 64., 102.),
                Step::Curve(61., 108., 54., 112.),
                Step::Close,
            ]),
            0xe9eaed,
            1.,
        ),
        // Nothing drawn on top of the hair. A parting line reads as a
        // scratch at this size and a shaded sweep reads as a patch of
        // something else; the silhouette is already saying which way it
        // goes, and it is the only part of it anybody looks at.
        // Sideburns down in front of each ear.
        tint(Shape::Outline(&[Step::Start(54., 112.), Step::Curve(52., 124., 58., 134.), Step::Curve(64., 128., 64., 112.), Step::Curve(64., 100., 66., 94.), Step::Curve(57., 100., 54., 112.), Step::Close]), 0xd7dade, 1.),
        tint(Shape::Outline(&[Step::Start(186., 112.), Step::Curve(188., 124., 182., 134.), Step::Curve(176., 128., 176., 112.), Step::Curve(176., 100., 174., 94.), Step::Curve(183., 100., 186., 112.), Step::Close]), 0xd7dade, 1.),

        // Heavy lids and the bags under them: the eyes are hooded and tired,
        // and drawing them wide and bright was half of why it read as
        // somebody else entirely.
        tint(Shape::Outline(&[Step::Start(74., 112.), Step::Curve(92., 104., 110., 112.), Step::Curve(92., 110., 74., 112.), Step::Close]), 0xd3a179, 1.),
        tint(Shape::Outline(&[Step::Start(166., 112.), Step::Curve(148., 104., 130., 112.), Step::Curve(148., 110., 166., 112.), Step::Close]), 0xd3a179, 1.),

        // Brows: low, straight, still dark, and closer together than they
        // were.
        tint(
            Shape::Outline(&[Step::Start(74., 104.), Step::Curve(90., 96., 110., 103.), Step::Line(109., 110.), Step::Curve(91., 104., 76., 111.), Step::Close]),
            0x6f6b64,
            1.,
        ),
        tint(
            Shape::Outline(&[Step::Start(166., 104.), Step::Curve(150., 96., 130., 103.), Step::Line(131., 110.), Step::Curve(149., 104., 164., 111.), Step::Close]),
            0x6f6b64,
            1.,
        ),

        // The eyes themselves, narrow.
        tint(Shape::Outline(&[Step::Start(80., 120.), Step::Curve(92., 112., 106., 119.), Step::Curve(93., 127., 80., 120.), Step::Close]), 0xfbfbfa, 1.),
        tint(Shape::Outline(&[Step::Start(160., 120.), Step::Curve(148., 112., 134., 119.), Step::Curve(147., 127., 160., 120.), Step::Close]), 0xfbfbfa, 1.),
        tint(Shape::Round { x: 93., y: 119.5, radius: 4.2 }, 0x4a5f73, 1.),
        tint(Shape::Round { x: 147., y: 119.5, radius: 4.2 }, 0x4a5f73, 1.),
        tint(Shape::Round { x: 93., y: 119.5, radius: 1.9 }, 0x17171a, 1.),
        tint(Shape::Round { x: 147., y: 119.5, radius: 1.9 }, 0x17171a, 1.),
        // The bags.
        line(Shape::Outline(&[Step::Start(82., 128.), Step::Curve(93., 133., 105., 128.)]), 0xc79a74, 1.6, 0.55),
        line(Shape::Outline(&[Step::Start(158., 128.), Step::Curve(147., 133., 135., 128.)]), 0xc79a74, 1.6, 0.55),

        // A long, straight nose that comes down further than it did, with a
        // heavy tip.
        tint(
            Shape::Outline(&[
                Step::Start(117., 110.),
                Step::Curve(112., 134., 106., 150.),
                Step::Curve(110., 160., 120., 160.),
                Step::Curve(130., 160., 134., 150.),
                Step::Curve(128., 134., 123., 110.),
                Step::Close,
            ]),
            0xdeae84,
            1.,
        ),
        tint(Shape::Oval { x: 120., y: 155., across: 15., down: 7., turn: 0. }, 0xd2a179, 1.),

        // The folds from the nose to the corners of the mouth, deep.
        line(Shape::Outline(&[Step::Start(103., 152.), Step::Curve(97., 164., 100., 176.)]), CREASE, 2.8, 1.),
        line(Shape::Outline(&[Step::Start(137., 152.), Step::Curve(143., 164., 140., 176.)]), CREASE, 2.8, 1.),

        // A thin mouth, set, and turned down at the corners.
        tint(
            Shape::Outline(&[
                Step::Start(101., 170.),
                Step::Curve(120., 166., 139., 170.),
                Step::Curve(120., 176., 101., 170.),
                Step::Close,
            ]),
            0xa9705a,
            1.,
        ),
        line(Shape::Outline(&[Step::Start(101., 170.), Step::Curve(120., 175., 139., 170.)]), 0x8e5b49, 2.2, 1.),
        line(Shape::Outline(&[Step::Start(101., 170.), Step::Curve(99., 174., 100., 177.)]), 0x8e5b49, 2., 0.9),
        line(Shape::Outline(&[Step::Start(139., 170.), Step::Curve(141., 174., 140., 177.)]), 0x8e5b49, 2., 0.9),
        // The crease under the lip, and the weight of the chin.
        line(Shape::Outline(&[Step::Start(110., 182.), Step::Curve(120., 186., 130., 182.)]), CREASE, 2., 0.8),
    ],
};

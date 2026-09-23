//! What a frame of the cinema is made of: marks, each a quad on the screen
//! and the few numbers `film.wgsl` needs to draw what it is.
//!
//! The acts lay them out in a world of their own, in the screen's pixels,
//! and look at it through a `Cam`: the camera of the whole piece, and inside
//! it whatever turns or leans or waves. A mark is placed by taking its
//! centre and its two half-axes through the camera, so a flag that is
//! leaning, zoomed and shaken is still a quad and one draw.

use bytemuck::{Pod, Zeroable};

/// What each mark is, as the shader numbers them.
mod kind {
    pub const SHAPE: f32 = 0.;
    pub const GLOW: f32 = 1.;
    pub const STROKE: f32 = 2.;
    pub const MOTE: f32 = 3.;
    pub const CLOTH: f32 = 4.;
    pub const SPRITE: f32 = 5.;
    pub const EYE: f32 = 6.;
    pub const SKY: f32 = 7.;
    pub const SKYLINE: f32 = 8.;
    pub const RAYS: f32 = 9.;
    pub const LENS: f32 = 10.;
}

/// The most marks a frame holds. A burst of fireworks is the crowd; the
/// busiest frame is well under this.
pub const MOST: usize = 4096;

/// One mark, as the shader reads it: six vectors of four.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Mark {
    /// The centre on the screen, and the half-axis along the mark's own x.
    pub at: [f32; 4],
    /// The half-axis along its own y, its kind, and how much of it shows.
    pub across: [f32; 4],
    pub first: [f32; 4],
    pub second: [f32; 4],
    pub p: [f32; 4],
    pub q: [f32; 4],
}

/// A colour and how much of it there is, each nought to one.
pub type Colour = [f32; 4];

/// `0xRRGGBB` at `alpha`.
pub const fn hex(rgb: u32, alpha: f32) -> Colour {
    const fn channel(rgb: u32, shift: u32) -> f32 {
        ((rgb >> shift) & 0xff) as f32 / 255.
    }
    [channel(rgb, 16), channel(rgb, 8), channel(rgb, 0), alpha]
}

/// `colour` with only `share` of it there.
pub fn faded([r, g, b, a]: Colour, share: f32) -> Colour {
    [r, g, b, a * share]
}

/// A camera: where a point of the world lands on the screen. `x' = a x + c y
/// + e`, `y' = b x + d y + f`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cam {
    m: [f32; 6],
}

impl Cam {
    /// The screen as it is.
    pub const STILL: Cam = Cam { m: [1., 0., 0., 1., 0., 0.] };

    /// The world scaled by `scale` about `centre`, and moved by `shift`.
    pub fn zoom((cx, cy): (f32, f32), scale: f32, (sx, sy): (f32, f32)) -> Cam {
        Cam { m: [scale, 0., 0., scale, cx - cx * scale + sx, cy - cy * scale + sy] }
    }

    /// This camera, looking at something turned by `turn` full turns and
    /// stretched by `(across, down)`, both about `pivot`, in the world's own
    /// coordinates.
    pub fn about(&self, (px, py): (f32, f32), turn: f32, across: f32, down: f32) -> Cam {
        let (sin, cos) = (turn * std::f32::consts::TAU).sin_cos();
        // Moved to the pivot, stretched, turned, and moved back.
        let (a, b, c, d) = (cos * across, sin * across, -sin * down, cos * down);
        let e = px - a * px - c * py;
        let f = py - b * px - d * py;
        self.then(Cam { m: [a, b, c, d, e, f] })
    }

    /// This camera, looking at something turned about `pivot`.
    pub fn turned(&self, pivot: (f32, f32), turn: f32) -> Cam {
        self.about(pivot, turn, 1., 1.)
    }

    /// `inner` seen through this: what is placed with `inner` first, then
    /// with this.
    fn then(&self, inner: Cam) -> Cam {
        let [a, b, c, d, e, f] = self.m;
        let [ia, ib, ic, id, ie, if_] = inner.m;
        Cam {
            m: [
                a * ia + c * ib,
                b * ia + d * ib,
                a * ic + c * id,
                b * ic + d * id,
                a * ie + c * if_ + e,
                b * ie + d * if_ + f,
            ],
        }
    }

    pub fn point(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.m;
        (a * x + c * y + e, b * x + d * y + f)
    }

    pub fn vector(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let [a, b, c, d, ..] = self.m;
        (a * x + c * y, b * x + d * y)
    }

    /// How much bigger than the world things come out, on the whole.
    pub fn scale(&self) -> f32 {
        let [a, b, c, d, ..] = self.m;
        (a * d - b * c).abs().sqrt()
    }
}

/// How a box or an oval is painted.
#[derive(Clone, Copy, Debug)]
pub struct Fill {
    pub top: Colour,
    pub bottom: Colour,
    /// The corners' radius, in the world's pixels; ignored for an oval.
    pub radius: f32,
    /// An edge round it: its colour and width.
    pub edge: Option<(Colour, f32)>,
    /// How far its edge is blurred, in pixels.
    pub soft: f32,
}

impl Fill {
    pub fn flat(colour: Colour) -> Fill {
        Fill { top: colour, bottom: colour, radius: 0., edge: None, soft: 0. }
    }

    pub fn down(top: Colour, bottom: Colour) -> Fill {
        Fill { top, bottom, radius: 0., edge: None, soft: 0. }
    }

    pub fn rounded(self, radius: f32) -> Fill {
        Fill { radius, ..self }
    }

    pub fn edged(self, colour: Colour, width: f32) -> Fill {
        Fill { edge: Some((colour, width)), ..self }
    }

    pub fn blurred(self, soft: f32) -> Fill {
        Fill { soft, ..self }
    }
}

/// A stroke of light.
#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    pub tail: Colour,
    pub head: Colour,
    /// The core's radius and the glow's, in the world's pixels.
    pub width: f32,
    pub glow: f32,
    /// How strong the glow is.
    pub strength: f32,
    /// How much of it covers what is behind, rather than lighting it.
    pub solid: f32,
    /// How much of it is drawn: from and to, each nought to one.
    pub drawn: (f32, f32),
}

impl Stroke {
    pub fn light(colour: Colour, width: f32, glow: f32, strength: f32) -> Stroke {
        Stroke { tail: colour, head: colour, width, glow, strength, solid: 0., drawn: (0., 1.) }
    }
}

/// An eye.
#[derive(Clone, Copy, Debug)]
pub struct Eye {
    pub iris: Colour,
    pub skin: Colour,
    /// Nought shut, one open, a little more forced wide.
    pub open: f32,
    /// Where it looks, each -1 to 1.
    pub look: (f32, f32),
    pub blink: f32,
    /// How far the pupil has turned from round into a six-pointed star.
    pub star: f32,
    /// In a face, with lids of skin, or on its own.
    pub in_face: bool,
}

/// What a mark is drawn with, besides where it is: two colours, and the
/// numbers its kind reads.
struct Look {
    first: Colour,
    second: Colour,
    p: [f32; 4],
    q: [f32; 4],
}

impl Look {
    /// One colour, and `p`.
    fn of(colour: Colour, p: [f32; 4]) -> Look {
        Look { first: colour, second: colour, p, q: [0.; 4] }
    }
}

/// A frame's marks, in the order they are drawn: the first is furthest
/// back.
#[derive(Default)]
pub struct Marks {
    pub list: Vec<Mark>,
}

impl Marks {
    pub fn with_room() -> Marks {
        Marks { list: Vec::with_capacity(MOST) }
    }

    /// A mark whose own square is `(x, y, w, h)` in the world, as `cam` sees
    /// it.
    fn placed(&mut self, cam: &Cam, (x, y, w, h): (f32, f32, f32, f32), kind: f32, alpha: f32, look: Look) {
        if alpha <= 0.001 || self.list.len() >= MOST {
            return;
        }
        let centre = cam.point((x + w / 2., y + h / 2.));
        let u = cam.vector((w / 2., 0.));
        let v = cam.vector((0., h / 2.));
        let Look { first, second, p, q } = look;
        self.list.push(Mark { at: [centre.0, centre.1, u.0, u.1], across: [v.0, v.1, kind, alpha], first, second, p, q });
    }

    /// A mark over the whole screen, untouched by any camera.
    fn cover(&mut self, (width, height): (f32, f32), kind: f32, alpha: f32, look: Look) {
        self.placed(&Cam::STILL, (0., 0., width, height), kind, alpha, look);
    }

    pub fn rect(&mut self, cam: &Cam, rect: (f32, f32, f32, f32), fill: Fill, alpha: f32) {
        let scale = cam.scale();
        let (edge, width) = fill.edge.unwrap_or(([0.; 4], 0.));
        let p = [fill.radius * scale, width * scale, fill.soft * scale, 0.];
        self.placed(cam, rect, kind::SHAPE, alpha, Look { first: fill.top, second: fill.bottom, p, q: edge });
    }

    pub fn oval(&mut self, cam: &Cam, (x, y): (f32, f32), (across, down): (f32, f32), fill: Fill, alpha: f32) {
        let scale = cam.scale();
        let (edge, width) = fill.edge.unwrap_or(([0.; 4], 0.));
        let p = [0., width * scale, fill.soft * scale, 1.];
        self.placed(cam, (x - across, y - down, across * 2., down * 2.), kind::SHAPE, alpha, Look { first: fill.top, second: fill.bottom, p, q: edge });
    }

    /// Light round `(x, y)`, gone by `radius`. `falloff` is how much of it
    /// is gathered at the middle: small is a haze, large a point.
    pub fn glow(&mut self, cam: &Cam, (x, y): (f32, f32), radius: f32, colour: Colour, falloff: f32) {
        self.placed(cam, (x - radius, y - radius, radius * 2., radius * 2.), kind::GLOW, 1., Look::of(colour, [falloff, 0., 0., 0.]));
    }

    /// A stroke from `from` to `to`.
    pub fn stroke(&mut self, cam: &Cam, from: (f32, f32), to: (f32, f32), stroke: Stroke, alpha: f32) {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let length = (dx * dx + dy * dy).sqrt();
        if length < 0.001 {
            return;
        }
        let reach = stroke.width + stroke.glow;
        let middle = ((from.0 + to.0) / 2., (from.1 + to.1) / 2.);
        // The stroke's own square lies along it: turned to its direction
        // about its middle.
        let turn = dy.atan2(dx) / std::f32::consts::TAU;
        let along = cam.turned(middle, turn);
        let scale = cam.scale();
        let half = length / 2. + reach;
        let p = [stroke.width * scale, stroke.glow * scale, stroke.drawn.0, stroke.drawn.1];
        let q = [stroke.strength, stroke.solid, 0., 0.];
        self.placed(&along, (middle.0 - half, middle.1 - reach, half * 2., reach * 2.), kind::STROKE, alpha, Look { first: stroke.tail, second: stroke.head, p, q });
    }

    /// A mote, `blur` out of focus from nought to one.
    pub fn mote(&mut self, cam: &Cam, (x, y): (f32, f32), radius: f32, colour: Colour, blur: f32) {
        let p = [blur, 0.35 + blur * 0.4, 0., 0.];
        self.placed(cam, (x - radius, y - radius, radius * 2., radius * 2.), kind::MOTE, 1., Look::of(colour, p));
    }

    /// A flag of `cloth` and `blue` in `rect`, the wave at `phase`, standing
    /// `height` of the flag's height, and the sheen `sheen` across it.
    pub fn cloth(&mut self, cam: &Cam, (x, y, w, h): (f32, f32, f32, f32), (cloth, blue): (Colour, Colour), (phase, height): (f32, f32), sheen: (f32, f32), alpha: f32) {
        // Room round the flag for the wave to carry it into.
        let room = (0.02, height * 1.2);
        let grown = (x - w * room.0 / 2., y - h * room.1 / 2., w * (1. + room.0), h * (1. + room.1));
        self.placed(cam, grown, kind::CLOTH, alpha, Look { first: cloth, second: blue, p: [phase, height, room.0, room.1], q: [sheen.0, sheen.1, 0., 0.] });
    }

    /// The portrait, in `rect`, lit round its edge from `light` by `rim`.
    pub fn portrait(&mut self, cam: &Cam, rect: (f32, f32, f32, f32), rim: Colour, light: (f32, f32), alpha: f32) {
        self.placed(cam, rect, kind::SPRITE, alpha, Look::of(rim, [light.0, light.1, 0., 0.]));
    }

    pub fn eye(&mut self, cam: &Cam, (x, y, w, h): (f32, f32, f32, f32), eye: Eye, alpha: f32) {
        let p = [eye.open, eye.look.0, eye.look.1, if eye.in_face { 1. } else { 0. }];
        self.placed(cam, (x, y, w, h), kind::EYE, alpha, Look { first: eye.iris, second: eye.skin, p, q: [eye.blink, eye.star, 0., 0.] });
    }

    /// The night over the whole screen, deepest at the top, lit round
    /// `light` = `(x, y, reach, strength)`.
    pub fn sky(&mut self, screen: (f32, f32), (top, foot): (Colour, Colour), stars: f32, light: [f32; 4], alpha: f32) {
        self.cover(screen, kind::SKY, alpha, Look { first: top, second: foot, p: [screen.1, stars, 0., 0.], q: light });
    }

    /// Towers along the foot of the screen, standing on `ground`, `tallest`
    /// at the most, their roofs caught by `light` = `(x, strength)`.
    pub fn skyline(&mut self, screen: (f32, f32), (towers, windows): (Colour, Colour), (ground, tallest): (f32, f32), light: (f32, f32), alpha: f32) {
        let band = (0., ground - tallest - 8., screen.0, screen.1 - (ground - tallest - 8.));
        self.placed(&Cam::STILL, band, kind::SKYLINE, alpha, Look { first: towers, second: windows, p: [ground, tallest, 0., 0.], q: [light.0, 0., 0., light.1] });
    }

    /// Beams about `centre`, `count` of them, turned `turn` full turns and
    /// reaching `reach`.
    pub fn rays(&mut self, screen: (f32, f32), centre: (f32, f32), (count, turn, reach): (f32, f32, f32), colour: Colour) {
        let p = [centre.0, centre.1, turn * std::f32::consts::TAU, count];
        self.cover(screen, kind::RAYS, 1., Look { q: [reach, 0., 0., 0.], ..Look::of(colour, p) });
    }

    /// The lens over everything: `darken` at the corners, `grain`, the bars
    /// `bars` of the way in, a white `flash`; `roll` picks the grain.
    pub fn lens(&mut self, screen: (f32, f32), (darken, grain, bars, flash): (f32, f32, f32, f32), roll: f32, alpha: f32) {
        self.cover(screen, kind::LENS, alpha, Look { q: [roll, 0., 0., 0.], ..Look::of([0.; 4], [darken, grain, bars, flash]) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close((x, y): (f32, f32), (ex, ey): (f32, f32)) -> bool {
        (x - ex).abs() < 0.001 && (y - ey).abs() < 0.001
    }

    #[test]
    fn a_camera_zooms_about_its_centre_and_turns_about_a_pivot() {
        let cam = Cam::zoom((100., 50.), 2., (5., 0.));
        assert!(close(cam.point((100., 50.)), (105., 50.)), "the centre stays put, and moves by the shift");
        assert!(close(cam.point((110., 50.)), (125., 50.)));
        // A quarter turn about (10, 0): (20, 0) goes to (10, 10), y down.
        let turned = Cam::STILL.turned((10., 0.), 0.25);
        assert!(close(turned.point((20., 0.)), (10., 10.)), "{:?}", turned.point((20., 0.)));
        assert!(close(turned.point((10., 0.)), (10., 0.)), "the pivot stays where it is");
        // What is turned inside a zoom is turned, then zoomed.
        let both = cam.turned((100., 50.), 0.5);
        assert!(close(both.point((110., 50.)), (85., 50.)));
        assert!((both.scale() - 2.).abs() < 0.001);
    }

    #[test]
    fn a_stroke_lies_along_itself() {
        let mut marks = Marks::default();
        marks.stroke(&Cam::STILL, (0., 0.), (0., 100.), Stroke::light(hex(0xffffff, 1.), 2., 8., 1.), 1.);
        let mark = marks.list[0];
        assert!(close((mark.at[0], mark.at[1]), (0., 50.)), "centred on its middle");
        // Its own x runs down the screen, as long as half of it and the
        // glow round its end.
        assert!(close((mark.at[2], mark.at[3]), (0., 60.)), "{:?}", mark.at);
    }

    #[test]
    fn nothing_is_drawn_for_what_does_not_show() {
        let mut marks = Marks::default();
        marks.rect(&Cam::STILL, (0., 0., 10., 10.), Fill::flat(hex(0xffffff, 1.)), 0.);
        assert!(marks.list.is_empty());
    }
}

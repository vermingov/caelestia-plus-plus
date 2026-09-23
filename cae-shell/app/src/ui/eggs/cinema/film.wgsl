// The cinema, a mark at a time.
//
// Every mark is a quad: a centre and two half-axes on the screen, which say
// where the mark lies and how it is turned and stretched, and what it is.
// Each is worked out here, where it is drawn, from its own few numbers —
// a stroke of light, a mote, a waving flag, an eye, the sky — so nothing is
// kept on the card but a portrait, and a frame is a buffer of marks and one
// draw. Everything is premultiplied: a mark that covers what is behind it
// has alpha, and light that covers nothing is added to it with none.
//
// Written for Vulkan's own coordinates: y runs down the screen, and clip
// space the same way.

struct Film {
    size: vec2<f32>,
    // Seconds since the picture began, for what shimmers on its own.
    time: f32,
    // 1 where the surface encodes what it is given, which then has to be
    // linear light.
    linearise: f32,
};

var<immediate> film: Film;

@group(0) @binding(0) var portrait: texture_2d<f32>;
@group(0) @binding(1) var pixels: sampler;

// What the kinds are, as `marks.rs` numbers them.
const SHAPE: u32 = 0u;
const GLOW: u32 = 1u;
const STROKE: u32 = 2u;
const MOTE: u32 = 3u;
const CLOTH: u32 = 4u;
const SPRITE: u32 = 5u;
const EYE: u32 = 6u;
const SKY: u32 = 7u;
const SKYLINE: u32 = 8u;
const RAYS: u32 = 9u;
const LENS: u32 = 10u;

struct Mark {
    // The centre, and the half-axis along the mark's own x.
    @location(0) at: vec4<f32>,
    // The half-axis along its own y, what kind it is, and how much of it
    // shows.
    @location(1) across: vec4<f32>,
    @location(2) first: vec4<f32>,
    @location(3) second: vec4<f32>,
    @location(4) p: vec4<f32>,
    @location(5) q: vec4<f32>,
};

struct Out {
    @builtin(position) position: vec4<f32>,
    // Where in the mark this is, in pixels along its own axes from its
    // centre.
    @location(0) local: vec2<f32>,
    // The screen pixel, for what is laid over the screen as a whole.
    @location(1) screen: vec2<f32>,
    @location(2) @interpolate(flat) half: vec2<f32>,
    @location(3) @interpolate(flat) kind_alpha: vec2<f32>,
    @location(4) @interpolate(flat) first: vec4<f32>,
    @location(5) @interpolate(flat) second: vec4<f32>,
    @location(6) @interpolate(flat) p: vec4<f32>,
    @location(7) @interpolate(flat) q: vec4<f32>,
};

@vertex
fn place(@builtin(vertex_index) corner: u32, mark: Mark) -> Out {
    let u = mark.at.zw;
    let v = mark.across.xy;
    let half = vec2(length(u), length(v));
    // Two pixels of room round every mark, for its edge to soften into.
    let grow = vec2(1.0) + 2.0 / max(half, vec2(0.001));
    let corner_at = vec2(f32(corner & 1u) * 2.0 - 1.0, f32(corner >> 1u) * 2.0 - 1.0) * grow;
    let screen = mark.at.xy + u * corner_at.x + v * corner_at.y;
    var out: Out;
    out.position = vec4(screen / film.size * 2.0 - 1.0, 0.0, 1.0);
    out.local = corner_at * half;
    out.screen = screen;
    out.half = half;
    out.kind_alpha = mark.across.zw;
    out.first = mark.first;
    out.second = mark.second;
    out.p = mark.p;
    out.q = mark.q;
    return out;
}

// -- Light and colour ------------------------------------------------------

fn decoded(colour: vec3<f32>) -> vec3<f32> {
    if film.linearise > 0.5 {
        return pow(max(colour, vec3(0.0)), vec3(2.2));
    }
    return colour;
}

/// `colour` covering `cover` of the pixel, premultiplied.
fn paint(colour: vec3<f32>, cover: f32) -> vec4<f32> {
    return vec4(decoded(colour) * cover, cover);
}

/// Light added to whatever is there, covering nothing.
fn shine(colour: vec3<f32>, amount: f32) -> vec4<f32> {
    return vec4(decoded(colour) * amount, 0.0);
}

/// `top` over `under`, both premultiplied.
fn over(top: vec4<f32>, under: vec4<f32>) -> vec4<f32> {
    return top + under * (1.0 - top.a);
}

/// How much of a pixel lies inside a distance of `d`, with an edge `soft`
/// pixels wide at the least. Every distance here is in the screen's own
/// pixels already, so the least is one.
fn inside(d: f32, soft: f32) -> f32 {
    let edge = max(1.0, soft);
    return clamp(0.5 - d / edge, 0.0, 1.0);
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let at = fract(p);
    let smooth_at = at * at * (3.0 - 2.0 * at);
    let a = hash(cell);
    let b = hash(cell + vec2(1.0, 0.0));
    let c = hash(cell + vec2(0.0, 1.0));
    let d = hash(cell + vec2(1.0, 1.0));
    return mix(mix(a, b, smooth_at.x), mix(c, d, smooth_at.x), smooth_at.y);
}

// -- Distances -------------------------------------------------------------

fn round_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half.x, half.y));
    let q = abs(p) - half + r;
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn ellipse(p: vec2<f32>, radii: vec2<f32>) -> f32 {
    let k0 = length(p / radii);
    let k1 = length(p / (radii * radii));
    return k0 * (k0 - 1.0) / max(k1, 0.00001);
}

/// A triangle with its points `r` from its centre, one straight up.
fn triangle(p_in: vec2<f32>, r: f32) -> f32 {
    let k = sqrt(3.0);
    var p = vec2(abs(p_in.x) - r * k * 0.5, -p_in.y + r * 0.5);
    if p.x + k * p.y > 0.0 {
        p = vec2(p.x - k * p.y, -k * p.x - p.y) / 2.0;
    }
    p.x = p.x - clamp(p.x, -r * k, 0.0);
    return -length(p) * sign(p.y);
}

/// The outline of the six-pointed star, `width` thick.
fn hexagram(p: vec2<f32>, r: f32, width: f32) -> f32 {
    let up = abs(triangle(p, r));
    let down = abs(triangle(vec2(p.x, -p.y), r));
    return min(up, down) - width * 0.5;
}

// -- The kinds -------------------------------------------------------------

/// A box or an oval, filled top to bottom, with an edge drawn round it.
/// p: corner radius, edge width, softness, 1 for an oval.
/// q: the edge's colour and how much of it shows.
fn shape(local: vec2<f32>, half: vec2<f32>, first: vec4<f32>, second: vec4<f32>, p: vec4<f32>, q: vec4<f32>) -> vec4<f32> {
    var d: f32;
    if p.w > 0.5 {
        d = ellipse(local, half);
    } else {
        d = round_box(local, half, p.x);
    }
    let along = clamp(local.y / half.y * 0.5 + 0.5, 0.0, 1.0);
    let fill = mix(first, second, along);
    var colour = paint(fill.rgb, fill.a * inside(d, p.z));
    if p.y > 0.0 {
        let ring = inside(abs(d + p.y * 0.5) - p.y * 0.5, p.z) * q.a;
        colour = over(paint(q.rgb, ring), colour);
    }
    return colour;
}

/// Light falling away from the middle.
/// first: its colour and strength. p.x: how quickly it falls away.
fn glow(local: vec2<f32>, half: vec2<f32>, first: vec4<f32>, p: vec4<f32>) -> vec4<f32> {
    let r2 = dot(local / half, local / half);
    let fade = exp(-r2 * p.x) * clamp(1.0 - r2, 0.0, 1.0);
    return shine(first.rgb, first.a * fade);
}

/// A stroke of light: a line with round ends, bright in its core and
/// glowing round it, drawn from one end to the other as far as `p.w` and
/// from as far as `p.z`.
/// first, second: the colour at the tail and at the head.
/// p: core radius, glow radius, from, to. q.x: how strong the glow is,
/// q.y: how much it covers rather than lights.
fn stroke(local: vec2<f32>, half: vec2<f32>, first: vec4<f32>, second: vec4<f32>, p: vec4<f32>, q: vec4<f32>) -> vec4<f32> {
    let reach = p.x + p.y;
    let length_half = max(half.x - reach, 0.0);
    // `from` and `to` are words WGSL keeps for itself.
    let start = -length_half + 2.0 * length_half * p.z;
    let end = -length_half + 2.0 * length_half * p.w;
    if end <= start {
        return vec4(0.0);
    }
    let x = clamp(local.x, start, end);
    let d = length(vec2(local.x - x, local.y)) - p.x;
    let along = select(0.5, (x + length_half) / (2.0 * length_half), length_half > 0.0);
    let colour = mix(first, second, along);
    let core = inside(d, 0.0) * colour.a;
    let halo = exp(-max(d, 0.0) / max(p.y, 0.001) * 3.0) * q.x * colour.a * (1.0 - core);
    // The core is white-hot where the glow is strong, as a real light's is.
    let hot = mix(colour.rgb, vec3(1.0), 0.55 * q.x);
    return shine(hot, core * (1.0 - q.y)) + paint(hot, core * q.y) + shine(colour.rgb, halo);
}

/// A mote out of focus: a disc of light, softer the further it is from the
/// plane in focus, with the faint bright rim a lens gives it.
/// first: its colour and strength. p.x: how far out of focus, p.y: rim.
fn mote(local: vec2<f32>, half: vec2<f32>, first: vec4<f32>, p: vec4<f32>) -> vec4<f32> {
    let r = length(local) / half.x;
    let edge = mix(0.12, 0.75, p.x);
    let disc = smoothstep(1.0, 1.0 - edge, r);
    let rim = smoothstep(1.0 - edge * 1.6, 1.0 - edge * 0.4, r) * disc * p.y;
    let body = disc * (0.55 + 0.45 * (1.0 - r * r));
    return shine(first.rgb, first.a * (body + rim * 0.8));
}

/// A flag, waving: the cloth is found under each pixel by undoing the wave,
/// drawn with its stripes and its star there, and shaded by how the wave
/// turns it to the light.
/// first: the cloth. second: the blue.
/// p: the wave's phase, its height as a share of the flag's, and the room
/// round the flag in the mark as a share of it, across and down.
/// q: where the sheen is across the cloth (0 to 1), its strength.
fn cloth(local: vec2<f32>, half: vec2<f32>, first: vec4<f32>, second: vec4<f32>, p: vec4<f32>, q: vec4<f32>) -> vec4<f32> {
    let flag = half / (vec2(1.0) + p.zw);
    let across = local.x / flag.x * 0.5 + 0.5;
    let down_seen = local.y / flag.y * 0.5 + 0.5;
    // Pinned at the pole, free at the fly: the wave grows along the cloth.
    let free = clamp(across, 0.0, 1.0);
    let phase = p.x;
    let wave = sin(across * 7.0 - phase) * 0.7 + sin(across * 12.5 - phase * 1.63 + down_seen * 2.1) * 0.3;
    let lift = p.y * free * wave;
    let down = down_seen - lift;
    // The slope of the cloth, for its light: where it turns away it darkens,
    // where it faces the light it brightens.
    let slope = p.y * (cos(across * 7.0 - phase) * 4.9 + cos(across * 12.5 - phase * 1.63 + down_seen * 2.1) * 3.75) * free;
    let lit = clamp(0.86 + slope * 0.9, 0.55, 1.12);

    let px = vec2(across - 0.5, down - 0.5) * flag * 2.0;
    let d = round_box(px, flag, 3.0);
    let cover = inside(d, 0.0);
    if cover <= 0.0 {
        return vec4(0.0);
    }

    // Two stripes, and the star between them.
    let stripe = min(abs(down - 0.195) - 0.055, abs(down - 0.805) - 0.055) * flag.y * 2.0;
    let star_at = vec2(across - 0.5, down - 0.5) * flag * 2.0;
    let star = hexagram(star_at, flag.y * 0.3, flag.y * 0.045);
    let blue = max(inside(stripe, 0.0), inside(star, 0.0));
    var colour = mix(first.rgb, second.rgb, blue) * lit;

    // A sheen travelling across it, and the cloth's own weave in the light.
    let sheen = exp(-pow((across + down * 0.35 - q.x) / 0.09, 2.0)) * q.y;
    colour = colour + vec3(sheen) * (0.6 + 0.4 * lit);
    colour = colour * (0.97 + 0.03 * noise(local * 0.9));
    // The edges hem the cloth, a shade darker.
    let hem = inside(-d - 2.0, 0.0);
    colour = mix(colour * 0.82, colour, hem);
    return paint(colour, cover * first.a);
}

/// The portrait: the picture made for it, and a rim of light round its edge
/// on the side the light comes from.
/// first: the rim's colour and strength. p.xy: which way the light comes
/// from, in the picture's own pixels.
fn sprite(local: vec2<f32>, half: vec2<f32>, first: vec4<f32>, p: vec4<f32>) -> vec4<f32> {
    let at = local / half * 0.5 + 0.5;
    if any(at < vec2(0.0)) || any(at > vec2(1.0)) {
        return vec4(0.0);
    }
    let texel = textureSampleLevel(portrait, pixels, at, 0.0);
    let size = vec2<f32>(textureDimensions(portrait));
    let beyond = textureSampleLevel(portrait, pixels, at + p.xy / size, 0.0).a;
    let rim = texel.a * (1.0 - beyond) * first.a;
    return texel + shine(first.rgb, rim);
}

/// An eye: the white, shaded as the ball it is, the iris with its fibres,
/// the pupil, the light caught on it, and lids — dark edges on an eye on
/// its own, skin on one in a face.
/// first: the iris. second: the skin, for one in a face.
/// p: how open (0 to 1), where it looks across and down (-1 to 1), 1 for
/// one in a face. q.x: how far through a blink; q.y: how far the pupil has
/// turned from round into a six-pointed star.
fn eye(local: vec2<f32>, half: vec2<f32>, first: vec4<f32>, second: vec4<f32>, p: vec4<f32>, q: vec4<f32>) -> vec4<f32> {
    let squash = 1.0 - q.x * 0.9;
    let opening = vec2(half.x, max(half.y * clamp(p.x, 0.0, 1.2) * squash, 0.6));
    // An almond rather than an oval: corners that come to a point.
    let almond = vec2(local.x, local.y * (1.0 + 0.35 * pow(abs(local.x) / half.x, 2.0)));
    let d = ellipse(almond, opening);
    let open = inside(d, 0.0);
    let ball = ellipse(local, half);
    let in_face = p.w > 0.5;
    let whole = select(open, inside(ball, 0.0), in_face);
    if whole <= 0.0 {
        return vec4(0.0);
    }

    // The white, rounder than the opening: darker towards the corners and
    // under the upper lid.
    let across = local.x / half.x;
    var white = vec3(0.96, 0.95, 0.93) * (1.0 - 0.32 * across * across);
    let under_lid = smoothstep(-opening.y, -opening.y * 0.35, local.y);
    white = white * mix(0.72, 1.0, under_lid);

    let r_iris = half.y * 0.82;
    let centre = vec2(p.y * half.x * 0.38, p.z * half.y * 0.2);
    let from_iris = local - centre;
    let r = length(from_iris) / r_iris;
    let angle = atan2(from_iris.y, from_iris.x);
    let fibres = 0.75 + 0.25 * sin(angle * 38.0 + noise(vec2(angle * 6.0, r * 5.0)) * 5.0);
    var iris = first.rgb * fibres * mix(1.25, 0.55, smoothstep(0.35, 1.0, r));
    // A dark ring at its edge, and the pupil.
    iris = mix(iris, iris * 0.35, smoothstep(0.82, 1.0, r));
    // The pupil: round, or a star as far as `q.y` says — the one shape grown
    // out of the other, by their distances — lit along its edge as it goes.
    let round = length(from_iris) - r_iris * 0.4;
    let star_r = r_iris * 0.68;
    let star = min(triangle(from_iris, star_r), triangle(vec2(from_iris.x, -from_iris.y), star_r));
    let pupil_d = mix(round, star, q.y);
    iris = mix(iris, vec3(0.02, 0.02, 0.03), inside(pupil_d, 0.0));
    iris = iris + vec3(0.45, 0.62, 1.0) * exp(-abs(pupil_d) / (r_iris * 0.07)) * q.y * 0.8;
    let in_iris = inside((r - 1.0) * r_iris, 0.0);
    var colour = mix(white, iris, in_iris);
    // The window the light comes in by, caught on the cornea.
    let catch_light = smoothstep(0.2, 0.08, length(from_iris / r_iris - vec2(-0.35, -0.4)));
    colour = colour + vec3(catch_light * 0.9);
    colour = colour * mix(0.8, 1.0, under_lid);

    var seen = paint(colour, open);
    if in_face {
        // Skin over the rest of the ball: the lids, shaded as they fold back.
        let lid_light = mix(0.78, 1.02, smoothstep(-half.y, half.y * 0.4, -abs(local.y)));
        let lids = paint(second.rgb * lid_light, (1.0 - open) * inside(ball, 0.0));
        seen = over(lids, seen);
    }
    // The line of the lashes along the opening.
    let lashes = inside(abs(d) - 1.2 - 0.9 * smoothstep(0.0, -opening.y, local.y), 0.0);
    seen = over(paint(vec3(0.1, 0.07, 0.06), lashes * 0.85), seen);
    return seen;
}

/// The night, with its stars, lit from where the last burst is.
/// first: the top of the sky. second: its foot.
/// p: where the foot is, down the screen, and how many stars show.
/// q: the burst's place, its reach and how bright it is now.
fn sky(screen: vec2<f32>, first: vec4<f32>, second: vec4<f32>, p: vec4<f32>, q: vec4<f32>) -> vec4<f32> {
    let down = clamp(screen.y / max(p.x, 1.0), 0.0, 1.0);
    var colour = mix(first.rgb, second.rgb, pow(down, 1.6));
    // Stars on a grid of cells, one to a cell at most, each with its own
    // twinkle.
    let cell = floor(screen / 22.0);
    let chance = hash(cell);
    let at = (cell + vec2(hash(cell + 7.1), hash(cell + 3.7))) * 22.0;
    let twinkle = 0.6 + 0.4 * sin(film.time * (2.0 + chance * 5.0) + chance * 40.0);
    let star = smoothstep(1.6, 0.0, length(screen - at)) * step(1.0 - p.y, chance) * twinkle * (1.0 - down);
    colour = colour + vec3(0.8, 0.86, 1.0) * star;
    let lit = exp(-pow(length(screen - q.xy) / max(q.z, 1.0), 2.0)) * q.w;
    colour = colour + vec3(0.55, 0.62, 0.85) * lit * 0.22;
    return paint(colour, first.a);
}

/// A city along the foot of the screen: two rows of towers, the far one
/// paler, windows lit here and there, and roofs caught by the light of the
/// bursts above.
/// first: the near towers. second: the windows.
/// p: where the ground is, the tallest a tower can be.
/// q: the light above, its place across the screen and its strength.
fn skyline(screen: vec2<f32>, first: vec4<f32>, second: vec4<f32>, p: vec4<f32>, q: vec4<f32>) -> vec4<f32> {
    let ground = p.x;
    let tallest = p.y;
    var colour = vec4(0.0);
    for (var row = 0; row < 2; row = row + 1) {
        let far = f32(row == 0);
        let width = mix(64.0, 44.0, far);
        let shift = far * 23.0;
        let cell = floor((screen.x + shift) / width);
        let left = cell * width - shift;
        let gap = 4.0 + hash(vec2(cell, 9.0 + far)) * 8.0;
        let height = tallest * mix(0.25, 1.0, hash(vec2(cell, 1.0 + far))) * mix(1.0, 0.72, far);
        let top = ground - height;
        let in_x = step(left + gap, screen.x) * step(screen.x, left + width - gap);
        let in_tower = in_x * step(top, screen.y);
        if in_tower <= 0.0 {
            continue;
        }
        var tower = first.rgb * mix(1.0, 1.9, far);
        // Windows on a grid, some lit, each lit one with a flicker of its own.
        let window = vec2(screen.x - left - gap, screen.y - top);
        let grid = floor(window / vec2(9.0, 13.0));
        let in_pane = step(2.0, fract(window.x / 9.0) * 9.0) * step(3.0, fract(window.y / 13.0) * 13.0);
        let lit = step(0.72, hash(grid + vec2(cell * 17.0, far * 31.0))) * in_pane * (1.0 - far * 0.5);
        tower = mix(tower, second.rgb * (0.8 + 0.2 * sin(film.time * 3.0 + hash(grid) * 20.0)), lit * 0.85);
        // The roof caught by what is bursting above it.
        let roof = exp(-(screen.y - top) / 6.0) * q.w * exp(-pow((screen.x - q.x) / (tallest * 3.0), 2.0));
        tower = tower + vec3(0.6, 0.7, 1.0) * roof * 0.8;
        colour = over(paint(tower, in_tower * first.a), colour);
    }
    return colour;
}

/// Beams turning about a point, bright towards it and gone at the middle,
/// as a fan of light behind something bright.
/// first: their colour and strength. p: the point, how far it has turned,
/// how many beams. q.x: how far they reach.
fn rays(screen: vec2<f32>, first: vec4<f32>, p: vec4<f32>, q: vec4<f32>) -> vec4<f32> {
    let away = screen - p.xy;
    let distance = length(away);
    let angle = atan2(away.y, away.x) + p.z;
    let beams = pow(0.5 + 0.5 * cos(angle * p.w), 18.0);
    let reach = smoothstep(q.x, q.x * 0.15, distance) * smoothstep(0.0, q.x * 0.12, distance);
    return shine(first.rgb, first.a * beams * reach);
}

/// What the lens does to all of it: the corners darkened, grain over
/// everything, the bars top and bottom, and the white of a cut.
/// p: the darkening, the grain, how far the bars are in (0 to 1), the flash.
/// q.x: which scatter of grain this frame has.
fn lens(screen: vec2<f32>, p: vec4<f32>, q: vec4<f32>, alpha: f32) -> vec4<f32> {
    let at = screen / film.size - 0.5;
    let away = length(at * vec2(1.0, 0.82)) * 1.35;
    let dark = p.x * smoothstep(0.25, 1.05, away);
    var colour = vec4(0.0, 0.0, 0.0, dark);
    // Grain: mostly specks of dark, now and then a bright one, as film has.
    let speck = hash(floor(screen / 1.5) + vec2(q.x * 13.7, q.x * 7.3)) - 0.5;
    if speck < 0.0 {
        colour = over(vec4(0.0, 0.0, 0.0, -speck * p.y), colour);
    } else {
        colour = colour + shine(vec3(1.0), speck * p.y * 0.8);
    }
    colour = over(paint(vec3(1.0), p.w), colour);
    let bar = film.size.y * 0.085 * p.z;
    let in_bar = step(screen.y, bar) + step(film.size.y - bar, screen.y);
    colour = mix(colour, vec4(0.0, 0.0, 0.0, 1.0), clamp(in_bar, 0.0, 1.0));
    return colour * alpha;
}

@fragment
fn shade(in: Out) -> @location(0) vec4<f32> {
    let kind = u32(in.kind_alpha.x + 0.5);
    let alpha = in.kind_alpha.y;
    var colour = vec4(0.0);
    switch kind {
        case SHAPE: { colour = shape(in.local, in.half, in.first, in.second, in.p, in.q); }
        case GLOW: { colour = glow(in.local, in.half, in.first, in.p); }
        case STROKE: { colour = stroke(in.local, in.half, in.first, in.second, in.p, in.q); }
        case MOTE: { colour = mote(in.local, in.half, in.first, in.p); }
        case CLOTH: { colour = cloth(in.local, in.half, in.first, in.second, in.p, in.q); }
        case SPRITE: { colour = sprite(in.local, in.half, in.first, in.p); }
        case EYE: { colour = eye(in.local, in.half, in.first, in.second, in.p, in.q); }
        case SKY: { colour = sky(in.screen, in.first, in.second, in.p, in.q); }
        case SKYLINE: { colour = skyline(in.screen, in.first, in.second, in.p, in.q); }
        case RAYS: { colour = rays(in.screen, in.first, in.p, in.q); }
        case LENS: { return lens(in.screen, in.p, in.q, alpha); }
        default: {}
    }
    return colour * alpha;
}

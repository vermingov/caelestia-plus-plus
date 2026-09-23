// The helix's scene: the dark behind it, and its shapes over that.
//
// The processor works out where every shape is and sorts them from the far
// end to the near; each shape here is a quad just big enough for it, and a
// pixel is only worked on where a shape can reach. Written for Vulkan's own
// coordinates: y runs down the screen, and clip space the same way.

struct Scene {
    size: vec2<f32>,
    // Where the helix's axis crosses the screen, and which way it runs.
    axis_origin: vec2<f32>,
    axis_direction: vec2<f32>,
    // 1 where the surface encodes what it is given, which then has to be
    // linear light.
    linearise: f32,
    spare: f32,
    deep: vec4<f32>,
    primary: vec4<f32>,
    hot: vec4<f32>,
};

var<immediate> scene: Scene;

// Kinds of shape, as the processor lays them out; see `helix::Shape`. A
// link, kind 2, is a rod in one colour, which is what is left over.
const BALL: f32 = 0.0;
const PAIR: f32 = 1.0;
const MOTE: f32 = 3.0;

// The light comes from the upper left, and a little from in front.
const LIGHT: vec3<f32> = vec3<f32>(-0.48, -0.62, -0.62);

fn finish(colour: vec3<f32>, alpha: f32) -> vec4<f32> {
    if scene.linearise > 0.5 {
        return vec4<f32>(pow(max(colour, vec3<f32>(0.0)), vec3<f32>(2.2)), alpha);
    }
    return vec4<f32>(colour, alpha);
}

// Noise from a pixel's position: the same pixel always gets the same grain,
// so the dark does not shimmer from one frame to the next.
fn grain(at: vec2<f32>) -> f32 {
    var p = fract(at * vec2<f32>(0.1031, 0.1030));
    p += dot(p, p.yx + 33.33);
    return fract((p.x + p.y) * p.x);
}

// One triangle that covers the screen.
@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

// The dark the helix hangs in: the deep shade of the accent nearly to black,
// darker still at the corners, and lit faintly along the helix itself.
@fragment
fn backdrop(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = scene.size;
    let dark = scene.deep.rgb * 0.07 + 0.012;

    let from_axis = at.xy - scene.axis_origin;
    let across = (from_axis.x * scene.axis_direction.y - from_axis.y * scene.axis_direction.x) / size.y;
    let along = dot(from_axis, scene.axis_direction) / size.x;
    let glow = exp(-across * across * 11.0) * exp(-along * along * 1.6);

    let centred = (at.xy / size - 0.5) * vec2<f32>(size.x / size.y, 1.0);
    let vignette = 1.0 - smoothstep(0.3, 1.1, length(centred));

    var colour = dark * (0.45 + 0.55 * vignette);
    colour += mix(scene.deep.rgb, scene.primary.rgb, 0.35) * 0.075 * glow;
    colour += (grain(at.xy) - 0.5) / 255.0;
    return finish(colour, 1.0);
}

struct Shape {
    @location(0) head: vec4<f32>,
    @location(1) tail: vec4<f32>,
    @location(2) first: vec4<f32>,
    @location(3) second: vec4<f32>,
};

struct Covered {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) head: vec4<f32>,
    @location(1) @interpolate(flat) tail: vec4<f32>,
    @location(2) @interpolate(flat) first: vec4<f32>,
    @location(3) @interpolate(flat) second: vec4<f32>,
};

// How far past its radius a shape's light reaches: a bead glows, a rod
// hardly, a mote not at all.
fn halo(kind: f32) -> f32 {
    if kind == BALL {
        return 2.3;
    }
    if kind == MOTE {
        return 1.0;
    }
    return 1.35;
}

// The quad a shape is drawn on: along the rod from end to end and across it,
// both widened by as far as the shape reaches. Corners in strip order.
@vertex
fn outline(@builtin(vertex_index) corner: u32, shape: Shape) -> Covered {
    let reach = halo(shape.first.w);
    let spread = max(shape.head.z * reach + shape.head.w, shape.tail.z * reach + shape.tail.w) + 1.5;
    let span = shape.tail.xy - shape.head.xy;
    let extent = length(span);
    var along = vec2<f32>(1.0, 0.0);
    if extent > 0.001 {
        along = span / extent;
    }
    let across = vec2<f32>(-along.y, along.x);
    var at = shape.head.xy - along * spread;
    if (corner & 1u) == 1u {
        at = shape.tail.xy + along * spread;
    }
    if (corner & 2u) == 2u {
        at += across * spread;
    } else {
        at -= across * spread;
    }
    let clip = at / scene.size * 2.0 - 1.0;
    return Covered(vec4<f32>(clip, 0.0, 1.0), shape.head, shape.tail, shape.first, shape.second);
}

@fragment
fn shade(covered: Covered) -> @location(0) vec4<f32> {
    let p = covered.position.xy;
    let a = covered.head;
    let b = covered.tail;
    let kind = covered.first.w;

    // The nearest point of the rod's axis — for a bead, its centre — and how
    // big and how blurred the shape is there.
    let span = b.xy - a.xy;
    let length2 = dot(span, span);
    var t = 0.0;
    if length2 > 0.0001 {
        t = clamp(dot(p - a.xy, span) / length2, 0.0, 1.0);
    }
    let offset = p - (a.xy + span * t);
    let apart = length(offset);
    let radius = max(mix(a.z, b.z, t), 0.001);
    let blur = mix(a.w, b.w, t);

    // How much of the pixel the shape covers: its edge softened by a pixel,
    // and by however far out of focus it is. Blurred, the same light is
    // spread over more of the screen, and each part of it is dimmer.
    let soft = blur * 0.5 + 0.7;
    let cover = 1.0 - smoothstep(radius - soft, radius + soft, apart);
    let spread = radius / (radius + blur * 0.5);

    if kind == MOTE {
        // Light in the air, unlit: mostly added, a little covering.
        let strength = covered.second.w * spread;
        return finish(covered.first.rgb * cover * strength, cover * strength * 0.25);
    }

    // The surface under the pixel: a sphere for a bead, a cylinder for a
    // rod, facing the eye at the middle and turning away to the edge.
    let inside = min(apart / radius, 1.0);
    var flat_normal = vec2<f32>(0.0);
    if apart > 0.0001 {
        flat_normal = offset / apart * inside;
    }
    let normal = vec3<f32>(flat_normal, -sqrt(max(1.0 - inside * inside, 0.0)));

    var base = mix(covered.first.rgb, covered.second.rgb, t);
    if kind == PAIR {
        // A base from each strand, and the bond between them a dark seam.
        base = select(covered.first.rgb, covered.second.rgb, t > 0.5);
        base *= 1.0 - 0.55 * (1.0 - smoothstep(0.012, 0.03, abs(t - 0.5)));
    }

    let light = normalize(LIGHT);
    let diffuse = max(dot(normal, light), 0.0);
    let halfway = normalize(light + vec3<f32>(0.0, 0.0, -1.0));
    let shine = pow(max(dot(normal, halfway), 0.0), 42.0);
    let rim = pow(1.0 - abs(normal.z), 3.0);
    let lit = base * (0.16 + 0.9 * diffuse) + scene.hot.rgb * (shine * 0.5 + rim * 0.3);

    // A bead glows: light past its edge, added and covering nothing.
    var glow = 0.0;
    if kind == BALL {
        let beyond = max(apart - radius, 0.0) / (radius * 0.9 + blur * 0.5);
        glow = exp(-beyond * beyond * 2.4) * 0.15 * (1.0 - cover);
    }

    let alpha = cover * spread;
    return finish(lit * alpha + base * glow * spread, alpha);
}

// An animated double helix on near-black, in three colours it is given.
//
// Drawn at half the screen's size and stretched by the compositor, so it is
// kept to arithmetic: no textures, no loops, two helices and a layer of dust.

struct Params {
    time: f32,
    aspect: f32,
    // 1 where the surface expects linear light and encodes it itself: what
    // is worked out below is already what the screen should show.
    linearise: f32,
    colour_deep: vec4<f32>,
    colour_main: vec4<f32>,
    colour_hot: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;

const PI: f32 = 3.14159265;

struct Corner {
    @builtin(position) position: vec4<f32>,
    @location(0) at: vec2<f32>,
};

// One triangle that covers the screen, with (0, 0) at the top left of it.
@vertex
fn corner(@builtin(vertex_index) index: u32) -> Corner {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return Corner(vec4<f32>(x, y, 0.0, 1.0), vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5));
}

fn hash(at: vec2<f32>) -> f32 {
    var p = fract(at * vec2<f32>(234.34, 435.345));
    p += dot(p, p + 34.23);
    return fract(p.x * p.y);
}

// `smoothstep` with its edges either way round, which the built-in one does
// not promise.
fn ramp(start: f32, end: f32, x: f32) -> f32 {
    let t = clamp((x - start) / (end - start), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// One double helix along +x of p, to be added to what is behind it.
// amp: strand radius, thick: core half-width, glow_k: halo falloff
fn helix(p: vec2<f32>, t: f32, amp: f32, thick: f32, glow_k: f32, bright: f32) -> vec3<f32> {
    let phase = p.x * 6.0 + t;
    let y1 = sin(phase) * amp;          // strand 1; strand 2 is half a turn on: -y1
    let depth1 = cos(phase) * 0.5 + 0.5; // 0 at the back, 1 at the front
    let depth2 = 1.0 - depth1;

    let d1 = abs(p.y - y1);
    let d2 = abs(p.y + y1);

    // The strand in front is thicker and brighter, the one behind thin and dim.
    let w1 = thick * mix(0.55, 1.15, depth1);
    let w2 = thick * mix(0.55, 1.15, depth2);
    var core1 = ramp(w1, w1 * 0.35, d1);
    var core2 = ramp(w2, w2 * 0.35, d2);
    let glow1 = exp(-d1 * d1 * glow_k);
    let glow2 = exp(-d2 * d2 * glow_k);

    // Whichever strand is in front eats the other where they cross.
    let front1 = step(depth2, depth1);
    core2 *= 1.0 - core1 * front1 * 0.85;
    core1 *= 1.0 - core2 * (1.0 - front1) * 0.85;

    let deep = params.colour_deep.rgb;
    let primary = params.colour_main.rgb;
    let hot = params.colour_hot.rgb;
    let c1 = mix(deep, primary, depth1) + hot * pow(depth1, 3.0) * 0.55;
    let c2 = mix(deep, primary, depth2) + hot * pow(depth2, 3.0) * 0.55;

    var colour = c1 * (core1 * (0.5 + 0.9 * depth1) + glow1 * 0.35 * (0.3 + 0.7 * depth1))
               + c2 * (core2 * (0.5 + 0.9 * depth2) + glow2 * 0.35 * (0.3 + 0.7 * depth2));

    // The rungs: ten base pairs a turn, gone where the strands cross.
    let cell = phase / (PI * 0.2);
    let rung_x = abs(fract(cell) - 0.5) * (PI * 0.2) / 6.0;
    let lowest = min(y1, -y1);
    let highest = max(y1, -y1);
    let inside = ramp(-0.004, 0.004, p.y - lowest) * ramp(-0.004, 0.004, highest - p.y);
    let rung = ramp(thick * 0.65, thick * 0.22, rung_x) * inside;
    let apart = ramp(0.15, 0.55, (highest - lowest) / (2.0 * amp));
    let tint = hash(vec2<f32>(floor(cell), 17.0));
    let rung_colour = mix(deep * 1.7, primary * 0.85, 0.3 + 0.5 * tint);
    colour += rung_colour * rung * apart * 0.8;

    return colour * bright;
}

@fragment
fn paint(corner: Corner) -> @location(0) vec4<f32> {
    let p = vec2<f32>((corner.at.x - 0.5) * params.aspect, corner.at.y - 0.5);
    let t = params.time;

    // The axis leans a little.
    let lean = -0.30;
    let turn = mat2x2<f32>(cos(lean), -sin(lean), sin(lean), cos(lean));
    let q = turn * p;

    // Behind it all: a red-black vignette, and a faint bloom along the axis.
    var colour = mix(vec3<f32>(0.051, 0.033, 0.035), vec3<f32>(0.014, 0.008, 0.010), ramp(0.15, 0.85, length(p)));
    colour += params.colour_main.rgb * 0.08 * exp(-q.y * q.y * 9.0);

    // A far helix for depth: smaller, slower, dim.
    let far = turn * (p * 1.9 + vec2<f32>(0.35, 0.22));
    colour += helix(far, t * 0.21 + 2.7, 0.11, 0.012, 2600.0, 0.28);

    colour += helix(q, t * 0.35, 0.17, 0.020, 900.0, 1.0);

    // Embers adrift, few and twinkling.
    let grid = p * 6.0 + vec2<f32>(t * 0.03, t * 0.012);
    let cell = floor(grid);
    let chance = hash(cell);
    let nudge = vec2<f32>(hash(cell + 3.1), hash(cell + 7.7)) - 0.5;
    let away = length(fract(grid) - 0.5 - nudge * 0.7);
    let twinkle = 0.5 + 0.5 * sin(t * (0.4 + chance * 0.8) + chance * 40.0);
    colour += params.colour_main.rgb * exp(-away * away * 260.0) * twinkle * step(0.8, chance) * 0.10;

    // Glow on glow saturates gently, and a grain of noise keeps the dark
    // gradients from banding.
    colour = colour / (1.0 + colour * 0.35);
    colour += (hash(corner.at * 971.7 + fract(t)) - 0.5) / 255.0;

    if params.linearise > 0.5 {
        colour = pow(max(colour, vec3<f32>(0.0)), vec3<f32>(2.2));
    }
    return vec4<f32>(colour, 1.0);
}

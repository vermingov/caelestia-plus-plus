// A wallpaper, and while one takes over from another, both.
//
// Each is already cut to the size of the screen, so there is nothing to work
// out but how far the change has got.

struct Change {
    // 0 is all of the one before, 1 all of the one after.
    done: f32,
};

var<immediate> change: Change;

@group(0) @binding(0) var before: texture_2d<f32>;
@group(0) @binding(1) var after: texture_2d<f32>;
@group(0) @binding(2) var pixels: sampler;

struct Corner {
    @builtin(position) position: vec4<f32>,
    @location(0) at: vec2<f32>,
};

// One triangle that covers the screen, with (0, 0) at the top left of it.
@vertex
fn cover(@builtin(vertex_index) index: u32) -> Corner {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return Corner(vec4<f32>(x, y, 0.0, 1.0), vec2<f32>(x * 0.5 + 0.5, y * 0.5 + 0.5));
}

@fragment
fn paint(corner: Corner) -> @location(0) vec4<f32> {
    let was = textureSample(before, pixels, corner.at);
    let is = textureSample(after, pixels, corner.at);
    return vec4<f32>(mix(was.rgb, is.rgb, change.done), 1.0);
}

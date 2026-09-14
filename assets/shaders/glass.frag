#version 440

// Clear liquid glass, as one pass over a rounded rectangle.
//
// The compositor blurs whatever is behind the pane; this draws the pane
// itself, and a clear pane is nearly nothing: the interior carries only
// the sliver of alpha the compositor needs before it will blur a pixel. The
// glass is all edge — a lens band where light gathers along the lip, a rim
// where it wraps, a specular streak from a light up and to the left, and
// shadow where the lip turns away. Light is white and shadow is black, not
// theme colours: a tinted pane is coloured plastic. Colour fringing on the
// rim is what makes the edge read as refraction rather than a drawn line.
//
// Everything is analytic: no texture taps, no derivatives, no loops. The
// grain is a static hash so nothing shimmers frame to frame.

layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec2 uSize;      // pane size in pixels
    float uRadius;   // corner radius in pixels
    float uLift;     // even illumination over the whole pane, 0..1
    float uRim;      // rim and specular strength
    float uScrim;    // dark base under the lift, for legibility; 0 for clear glass
    float uLens;     // strength of the edge lensing and the shadow it casts
    float uGrain;    // grain amplitude
    float uBand;     // lens band depth in pixels
    vec4 uLight;     // what light looks like on this theme (on-surface)
    vec4 uDark;      // what shadow looks like (lowest surface container)
};

float hash(vec2 p) {
    p = fract(p * vec2(234.34, 435.345));
    p += dot(p, p + 34.23);
    return fract(p.x * p.y);
}

// Signed distance to a rounded rectangle centred on the origin: negative
// inside. b is the hs size, r the corner radius.
float roundedRect(vec2 p, vec2 b, float r) {
    vec2 q = abs(p) - b + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

// The outward normal of that shape at p, without derivatives: straight out
// from the flat sides, radial in the corners.
vec2 rimNormal(vec2 p, vec2 b, float r) {
    vec2 q = abs(p) - b + r;
    vec2 n = (q.x > 0.0 && q.y > 0.0) ? normalize(q) : ((q.x > q.y) ? vec2(1.0, 0.0) : vec2(0.0, 1.0));
    return n * sign(p + 1e-4);
}

void main() {
    vec2 px = qt_TexCoord0 * uSize;
    vec2 hs = uSize * 0.5;
    vec2 p = px - hs;
    float r = min(uRadius, min(hs.x, hs.y));

    float d = roundedRect(p, hs, r);
    float cover = clamp(0.5 - d, 0.0, 1.0); // one-pixel antialiased edge
    if (cover <= 0.0) {
        fragColor = vec4(0.0);
        return;
    }
    float e = max(-d, 0.0); // depth inward from the edge, in pixels

    // Which way this bit of edge faces, against a light up and to the left.
    vec2 n = rimNormal(p, hs, r);
    const vec2 L = normalize(vec2(-0.55, -0.83));
    float lit = dot(n, L) * 0.5 + 0.5;

    // Lens band: light gathering along the lip, strongest at the edge.
    float band = pow(clamp(1.0 - e / uBand, 0.0, 1.0), 2.4);
    // The rim itself, a pixel and a hs wide, with the colour channels
    // offset in and out so the line fringes like refracted light.
    float rimG = 1.0 - smoothstep(0.0, 1.6, e);
    float rimR = 1.0 - smoothstep(0.0, 1.6, e - 0.45);
    float rimB = 1.0 - smoothstep(0.0, 1.6, e + 0.45);
    // Specular streak just inside the top edge, peaking a third of the way
    // across and gone before the corners.
    float sx = qt_TexCoord0.x;
    float streak = exp(-pow((sx - 0.32) / 0.26, 2.0)) * exp(-pow((e - 1.4) / 1.8, 2.0)) * step(p.y, 0.0);
    // And a softer one along the lit part of the left edge.
    float sy = qt_TexCoord0.y;
    float streakL = exp(-pow((sy - 0.28) / 0.22, 2.0)) * exp(-pow((e - 1.4) / 1.6, 2.0)) * step(p.x, 0.0) * 0.6;

    // Grain: static, centred on zero, so it neither shimmers nor tints.
    float grain = (hash(floor(px)) - 0.5) * uGrain * band;

    vec3 light = uLight.rgb;
    vec3 dark = uDark.rgb;

    // Premultiplied accumulation: each layer is a tint at an alpha.
    vec3 rgb = vec3(0.0);
    float a = 0.0;

    // Scrim, then the even lift over it.
    a += uScrim;
    rgb += dark * uScrim;

    float lift = max(uLift + grain, 0.012);
    a += lift;
    rgb += light * lift;

    // Lensing: light on the lit lip, shadow on the far one.
    // Dark glass: the lit lip barely glows, the far lip mostly darkens.
    float lensLight = uLens * band * lit * 0.07;
    float lensDark = uLens * band * (1.0 - lit) * 0.18;
    a += lensLight + lensDark;
    rgb += light * lensLight + dark * lensDark;

    // The rim, fringed.
    float rimA = uRim * (0.04 + 0.12 * lit);
    vec3 rim = vec3(rimR, rimG, rimB) * rimA;
    a += rimG * rimA;
    rgb += light * rim;

    // Speculars.
    float spec = uRim * 0.14 * (streak + streakL);
    a += spec;
    rgb += light * spec;

    a = min(a, 1.0);
    fragColor = vec4(rgb, a) * cover * qt_Opacity;
}

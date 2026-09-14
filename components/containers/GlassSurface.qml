import QtQuick
import Caelestia.Config

// A pane of glass, drawn in one pass (assets/shaders/glass.frag).
//
// The compositor blurs what is behind it; this draws the pane: an even lift
// of light, a lens band where light gathers along the lip, a fringed rim, a
// specular streak from a light up and to the left, and a shadow on the lip
// that faces away. All of it is shaped by the distance to the edge, so it
// wraps the corners the way a real rim does.
//
// One draw call, no texture round trip, nothing animated: the grain is a
// static hash, and a pane that is not visible costs nothing at all.
ShaderEffect {
    id: root

    property real radius: Tokens.rounding.extraLarge

    // Even illumination over the whole pane. Zero is clear glass — the
    // shader keeps the sliver of alpha the compositor needs to blur behind
    // it. Raise it for a frosted, Apple-regular look.
    property real lift: 0.0

    // Rim and specular brightness.
    property real rim: 1.0

    // Dark base under the lift, for legibility over bright backdrops. Zero is
    // clear glass; anything above starts to look like smoked plastic.
    property real scrim: 0.0

    // Strength of the edge lensing and the shadow it casts on the far lip.
    property real lens: 1.0

    // How deep the lens band reaches in from the edge, in pixels.
    property real band: 14

    // Grain in the lens band: enough to break its gradient, not enough to
    // see as texture.
    property real grain: 0.05

    // White light and black shadow, whatever the theme: glass has no colour
    // of its own, and lighting it with the theme's text colour tints it.
    property color light: "#ffffff"
    property color dark: "#000000"

    // The shader's uniforms, by name.
    readonly property vector2d uSize: Qt.vector2d(width, height)
    readonly property real uRadius: radius
    readonly property real uLift: lift
    readonly property real uRim: rim
    readonly property real uScrim: scrim
    readonly property real uLens: lens
    readonly property real uGrain: grain
    readonly property real uBand: band
    readonly property color uLight: light
    readonly property color uDark: dark

    fragmentShader: Qt.resolvedUrl("../../assets/shaders/glass.frag.qsb")
}

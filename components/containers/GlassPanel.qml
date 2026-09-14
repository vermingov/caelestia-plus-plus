import QtQuick
import Caelestia.Config
import qs.components
import qs.components.effects
import qs.services

// The shell's glass material: a GlassSurface with the shell's own settings,
// clipping its children to its shape, with a shadow under it.
//
// Apple's Liquid Glass reads as glass because it *adds* light and gathers it
// at the edges. A dark scrim over a blur is a smoked panel, not glass — so
// the scrim here defaults to nothing and the work is done by the pane
// shader: an illumination lift, a lens band along the lip, a fringed rim and
// a specular streak. The trade: the brighter and clearer the pane, the harder
// light text is to read over a busy backdrop, so the three amounts come from
// ShellPrefs and can be dialled per wallpaper without touching this file.
StyledClippingRect {
    id: root

    property real rim: ShellPrefs.glassRim
    property real lift: ShellPrefs.glassLift
    property real scrim: ShellPrefs.glassScrim
    property bool shadow: true

    // The pane must never be fully transparent anywhere: the compositor
    // blurs only pixels with alpha, and a hole in the pane is a hole in the
    // frost. GlassSurface keeps a floor on its lift for the same reason.
    color: "transparent"
    radius: Tokens.rounding.extraLarge

    Elevation {
        anchors.fill: parent
        z: -1
        radius: root.radius
        level: root.shadow ? 3 : 0
        visible: root.shadow
    }

    GlassSurface {
        anchors.fill: parent
        z: -1
        radius: root.radius
        lift: root.lift
        rim: root.rim
        scrim: root.scrim
    }
}

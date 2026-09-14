import QtQuick
import Caelestia.Config
import qs.components
import qs.components.effects
import qs.services

// The shell's glass material: a frosted surface with a lit rim.
//
// What makes Apple's Liquid Glass read as glass is not the blur — it is the
// edge. Light catches the rim and falls off around the shape, which is what
// tells the eye there is a solid pane there rather than a smudge. So this
// draws three things over the compositor's blur: a barely-there fill, a
// uniform hairline rim, and a brighter specular streak along the top edge.
//
// The fill alpha must stay above the compositor's ignore_alpha cutoff (see
// Colours.reloadHyprRules) or Hyprland stops blurring behind it and the
// frost vanishes.
StyledClippingRect {
    id: root

    // How lit the edge is. 1 is a clear pane catching a light source above
    // it; lower it for surfaces that should sit back.
    property real rim: 1

    property bool shadow: true

    color: Colours.tPalette.m3surfaceContainer
    radius: Tokens.rounding.extraLarge

    Elevation {
        anchors.fill: parent
        z: -1
        radius: root.radius
        level: root.shadow ? 3 : 0
        visible: root.shadow
    }

    // Falls off down the pane: brightest where the light meets it, almost
    // gone by the bottom edge
    Rectangle {
        anchors.fill: parent

        radius: root.radius
        color: "transparent"
        border.width: 1

        gradient: Gradient {
            orientation: Gradient.Vertical

            GradientStop {
                position: 0
                color: Qt.alpha(Colours.palette.m3onSurface, 0.05 * root.rim)
            }
            GradientStop {
                position: 0.45
                color: "transparent"
            }
        }

        border.color: Qt.alpha(Colours.palette.m3onSurface, 0.12 * root.rim)
    }

    // The specular streak itself: a hairline just inside the top edge,
    // fading out before the corners so it reads as a highlight rather than
    // a drawn line
    Rectangle {
        anchors.top: parent.top
        anchors.topMargin: 1
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: root.radius * 0.6
        anchors.rightMargin: root.radius * 0.6

        implicitHeight: 1

        gradient: Gradient {
            orientation: Gradient.Horizontal

            GradientStop {
                position: 0
                color: "transparent"
            }
            GradientStop {
                position: 0.5
                color: Qt.alpha(Colours.palette.m3onSurface, 0.35 * root.rim)
            }
            GradientStop {
                position: 1
                color: "transparent"
            }
        }
    }
}

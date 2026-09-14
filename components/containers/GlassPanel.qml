import QtQuick
import Caelestia.Config
import qs.components
import qs.components.effects
import qs.services

// The shell's glass material.
//
// The thing that makes Apple's Liquid Glass read as glass is that it *adds*
// light. A dark scrim over a blur reads as a smoked panel; real glass lifts
// what is behind it and catches a highlight along its rim. So this paints,
// in order: a thin scrim that guarantees text contrast over a bright
// backdrop, an illumination gradient that lifts the whole pane, a hairline
// rim, and a specular streak where the light meets the top edge.
//
// Total fill alpha must stay above the compositor's ignore_alpha cutoff (see
// Colours.reloadHyprRules) or Hyprland stops blurring behind it and the
// frost vanishes entirely.
StyledClippingRect {
    id: root

    // Defaults come from ShellPrefs so the mix can be dialled live, without
    // a restart, in ~/.local/state/caelestia/prefs.json
    property real rim: ShellPrefs.glassRim
    property real lift: ShellPrefs.glassLift
    property real scrim: ShellPrefs.glassScrim

    property bool shadow: true

    // Only enough scrim to keep light text legible over a bright backdrop;
    // the illumination below does the visual work
    color: Qt.alpha(Colours.palette.m3surfaceContainerLowest, root.scrim)
    radius: Tokens.rounding.extraLarge

    Elevation {
        anchors.fill: parent
        z: -1
        radius: root.radius
        level: root.shadow ? 3 : 0
        visible: root.shadow
    }

    // Illumination: brightest where the light lands, falling off down the
    // pane so the surface has direction instead of reading as flat film
    Rectangle {
        anchors.fill: parent
        radius: root.radius

        gradient: Gradient {
            orientation: Gradient.Vertical

            GradientStop {
                position: 0
                color: Qt.alpha(Colours.palette.m3onSurface, root.lift * 1.35)
            }
            GradientStop {
                position: 0.55
                color: Qt.alpha(Colours.palette.m3onSurface, root.lift * 0.7)
            }
            GradientStop {
                position: 1
                color: Qt.alpha(Colours.palette.m3onSurface, root.lift * 0.5)
            }
        }
    }

    // The rim: light wrapping the edge of the pane
    Rectangle {
        anchors.fill: parent

        radius: root.radius
        color: "transparent"
        border.width: 1
        border.color: Qt.alpha(Colours.palette.m3onSurface, 0.22 * root.rim)
    }

    // Specular streak just inside the top edge, fading before the corners so
    // it reads as a highlight rather than a drawn line
    Rectangle {
        anchors.top: parent.top
        anchors.topMargin: 1
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: root.radius * 0.5
        anchors.rightMargin: root.radius * 0.5

        implicitHeight: 1

        gradient: Gradient {
            orientation: Gradient.Horizontal

            GradientStop {
                position: 0
                color: "transparent"
            }
            GradientStop {
                position: 0.5
                color: Qt.alpha(Colours.palette.m3onSurface, 0.5 * root.rim)
            }
            GradientStop {
                position: 1
                color: "transparent"
            }
        }
    }
}

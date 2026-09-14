import QtQuick
import Caelestia.Config
import qs.components
import qs.components.effects
import qs.services

// The shell's glass material.
//
// Apple's Liquid Glass reads as glass because it *adds* light and gathers it
// at the edges. A dark scrim over a blur is a smoked panel, not glass — so
// the scrim here defaults to nothing and the work is done by an illumination
// layer that lifts the blurred backdrop, a lensing band where light collects
// along the top and bottom edges, a bright rim, and a specular streak.
//
// The trade this makes: the brighter and clearer the pane, the harder light
// text is to read over a busy backdrop. All three amounts come from
// ShellPrefs and hot-reload, so the balance can be dialled per wallpaper
// without touching this file.
StyledClippingRect {
    id: root

    // Brightness of the rim and the specular streak.
    property real rim: ShellPrefs.glassRim

    // Lifts the blurred backdrop. This is what makes the pane read as glass
    // rather than as a darker rectangle.
    property real lift: ShellPrefs.glassLift

    // Optional darkening under the lift, for legibility over bright
    // backdrops. Zero by default: it is the thing that makes glass look
    // like smoked plastic.
    property real scrim: ShellPrefs.glassScrim

    property bool shadow: true

    readonly property color light: Colours.palette.m3onSurface

    color: root.scrim > 0 ? Qt.alpha(Colours.palette.m3surfaceContainerLowest, root.scrim) : "transparent"
    radius: Tokens.rounding.extraLarge

    Elevation {
        anchors.fill: parent
        z: -1
        radius: root.radius
        level: root.shadow ? 3 : 0
        visible: root.shadow
    }

    // Illumination: an even lift across the pane, strongest where the light
    // lands so the surface has a direction
    Rectangle {
        anchors.fill: parent
        radius: root.radius

        gradient: Gradient {
            orientation: Gradient.Vertical

            GradientStop {
                position: 0
                color: Qt.alpha(root.light, root.lift * 1.2)
            }
            GradientStop {
                position: 0.5
                color: Qt.alpha(root.light, root.lift * 0.75)
            }
            GradientStop {
                position: 1
                color: Qt.alpha(root.light, root.lift)
            }
        }
    }

    // Lensing: light collecting along the top and bottom edges and falling
    // away through the middle, which is the cue that the pane has thickness
    Rectangle {
        anchors.fill: parent
        radius: root.radius

        gradient: Gradient {
            orientation: Gradient.Vertical

            GradientStop {
                position: 0
                color: Qt.alpha(root.light, 0.22 * root.rim)
            }
            GradientStop {
                position: 0.12
                color: "transparent"
            }
            GradientStop {
                position: 0.88
                color: "transparent"
            }
            GradientStop {
                position: 1
                color: Qt.alpha(root.light, 0.16 * root.rim)
            }
        }
    }

    // The rim: light wrapping the edge of the pane
    Rectangle {
        anchors.fill: parent

        radius: root.radius
        color: "transparent"
        border.width: 1
        border.color: Qt.alpha(root.light, 0.35 * root.rim)
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
                color: Qt.alpha(root.light, 0.6 * root.rim)
            }
            GradientStop {
                position: 1
                color: "transparent"
            }
        }
    }
}

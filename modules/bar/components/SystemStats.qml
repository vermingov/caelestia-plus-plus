pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Caelestia.Config
import Caelestia.Services
import qs.components
import qs.components.controls
import qs.services

// Live CPU / RAM / GPU readout pill. Each metric is its own cell, toggled
// individually from nexus > taskbar > system monitor; a cell collapses with
// a width+fade animation instead of popping out. Pollers are refcounted per
// visible cell, so a disabled metric costs nothing.
StyledRect {
    id: root

    implicitWidth: row.implicitWidth + Tokens.padding.medium * 2
    implicitHeight: Tokens.sizes.bar.innerWidth

    color: Qt.alpha(Colours.tPalette.m3surfaceContainer, Colours.tPalette.m3surfaceContainer.a * 0.7)
    radius: Tokens.rounding.full
    border.width: 1
    border.color: Qt.alpha(Colours.palette.m3outlineVariant, 0.4)
    clip: true
    scale: hover.hovered ? 1.05 : 1

    HoverHandler {
        id: hover
    }

    Behavior on scale {
        Anim {
            type: Anim.FastSpatial
        }
    }

    Behavior on implicitWidth {
        Anim {
            type: Anim.Emphasized
        }
    }

    RowLayout {
        id: row

        anchors.centerIn: parent
        spacing: Tokens.spacing.small

        StatCell {
            shown: ShellPrefs.barShowCpu
            label: qsTr("CPU")
            accent: Colours.palette.m3primary
            service: Cpu
        }

        StatCell {
            shown: ShellPrefs.barShowRam
            label: qsTr("RAM")
            accent: Colours.palette.m3tertiary
            service: Memory
        }

        StatCell {
            shown: ShellPrefs.barShowGpu && Gpu.type !== Gpu.None
            label: qsTr("GPU")
            accent: Colours.palette.m3secondary
            service: Gpu
        }
    }

    component StatCell: Item {
        id: cell

        required property bool shown
        required property string label
        required property color accent
        required property var service
        // Whole percents only: sub-percent jitter would restart both
        // animations every sample, and any running animation renders the
        // fullscreen drawers window at 60 fps
        readonly property real value: isNaN(service.percentage) ? NaN : Math.round(service.percentage * 100) / 100
        // Danger tint well before saturation so a pegged core is obvious at a glance
        readonly property color colour: !isNaN(value) && value >= 0.9 ? Colours.palette.m3error : accent

        Layout.alignment: Qt.AlignVCenter

        implicitWidth: shown ? content.implicitWidth : 0
        implicitHeight: content.implicitHeight
        opacity: shown ? 1 : 0
        // Fully collapsed cells drop out of the row so their spacing goes too
        visible: implicitWidth > 0
        // Reveal-style collapse: content clips at the shrinking edge instead
        // of sliding over the neighbour cell
        clip: true

        Behavior on implicitWidth {
            Anim {
                type: Anim.Emphasized
            }
        }

        Behavior on opacity {
            Anim {
                type: Anim.DefaultEffects
            }
        }

        Loader {
            id: content

            anchors.verticalCenter: parent.verticalCenter
            // Keep content alive through the collapse animation
            active: cell.shown || cell.opacity > 0

            sourceComponent: RowLayout {
                spacing: Tokens.spacing.extraSmall

                ServiceRef {
                    service: cell.service
                }

                CircularProgress {
                    Layout.alignment: Qt.AlignVCenter
                    implicitSize: Tokens.sizes.bar.innerWidth - Tokens.padding.small * 2
                    strokeWidth: 2
                    hasEndIndicator: false
                    value: cell.value
                    // The clamped minimum arc renders as a stray dot at 0%;
                    // fade the arc out instead of showing the sliver
                    fgColour: cell.value >= 0.005 ? cell.colour : "transparent"
                    bgColour: Qt.alpha(Colours.palette.m3outlineVariant, 0.35)
                    // Wave only under the cursor: an idle infinite animation
                    // would force 60fps bar re-renders forever (see Clock)
                    wavy: hover.hovered

                    Behavior on clampedVal {
                        Anim {
                            type: Anim.FastEffects
                        }
                    }
                }

                StyledText {
                    Layout.alignment: Qt.AlignVCenter
                    text: cell.label
                    color: cell.accent
                    font: Tokens.font.body.builders.small.scale(0.8).weight(Font.Bold).build()
                }

                StyledText {
                    // Count toward each new sample digit by digit; a text-swap
                    // animation would re-fade the whole readout every tick
                    property real displayed: isNaN(cell.value) ? 0 : cell.value

                    Layout.alignment: Qt.AlignVCenter
                    // Reserve the widest possible reading so the pill never
                    // resizes, even when a value hits 100%
                    Layout.preferredWidth: metrics.width
                    text: isNaN(cell.value) ? "…" : Math.round(displayed * 100) + "%"

                    Behavior on displayed {
                        Anim {
                            type: Anim.FastEffects
                        }
                    }
                    color: cell.colour
                    font: Tokens.font.body.builders.small.scale(0.85).build()

                    TextMetrics {
                        id: metrics

                        font: Tokens.font.body.builders.small.scale(0.85).build()
                        text: "100%"
                    }
                }
            }
        }
    }
}

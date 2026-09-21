pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Caelestia.Config
import qs.components
import qs.components.containers
import qs.components.controls
import qs.services

// Feature-mode menu, toggled by the bar wrench. One switch row per entry in
// Features.features. Dismiss: click the scrim or press Escape.
Scope {
    id: root

    // Not while the external bar draws its own.
    readonly property bool open: Features.menuOpen && !ExternalBar.hasFeatures
    readonly property var focusedScreen: Quickshell.screens.find(s => s.name === Hypr.focusedMonitor?.name) ?? Quickshell.screens[0]

    StyledWindow {
        id: win

        screen: root.focusedScreen
        name: "features-menu"
        visible: root.open || closeTimer.running

        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.exclusionMode: ExclusionMode.Ignore
        WlrLayershell.keyboardFocus: root.open ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

        anchors.top: true
        anchors.bottom: true
        anchors.left: true
        anchors.right: true

        Timer {
            id: closeTimer
            interval: Tokens.anim.durations.large
        }

        Connections {
            target: root
            function onOpenChanged(): void {
                if (!root.open)
                    closeTimer.restart();
            }
        }

        StyledRect {
            anchors.fill: parent
            color: Colours.palette.m3scrim
            opacity: root.open ? 0.25 : 0

            Behavior on opacity {
                Anim {
                    type: Anim.DefaultEffects
                }
            }

            MouseArea {
                anchors.fill: parent
                onClicked: Features.menuOpen = false
            }
        }

        // Outer bezel: translucent tinted shell with a hairline; the solid
        // core sits inset for concentric corners.
        StyledRect {
            id: card

            // Sits under the right bar cluster, where the wrench lives.
            anchors.right: parent.right
            anchors.top: parent.top
            // Below the floating bar pill: pill height + its float margins + gap
            anchors.topMargin: Tokens.sizes.bar.innerWidth + Math.max(Tokens.padding.small, Config.border.thickness) * 2 + Tokens.padding.large * 3
            anchors.rightMargin: Tokens.padding.large * 2

            implicitWidth: 380
            implicitHeight: core.implicitHeight + Tokens.padding.small * 2

            radius: Tokens.rounding.large
            color: Qt.alpha(Colours.palette.m3surfaceContainer, 0.7)
            border.width: 1
            border.color: Qt.alpha(Colours.palette.m3outlineVariant, 0.4)

            opacity: root.open ? 1 : 0
            scale: root.open ? 1 : 0.94
            focus: root.open

            Keys.onEscapePressed: Features.menuOpen = false

            transform: Translate {
                y: root.open ? 0 : -30

                Behavior on y {
                    Anim {
                        type: Anim.Emphasized
                    }
                }
            }

            Behavior on opacity {
                Anim {
                    type: Anim.DefaultEffects
                }
            }

            Behavior on scale {
                Anim {
                    type: Anim.Emphasized
                }
            }

            StyledClippingRect {
                id: core

                anchors.fill: parent
                anchors.margins: Tokens.padding.small

                implicitHeight: inner.implicitHeight
                radius: card.radius - Tokens.padding.small
                color: Colours.palette.m3surfaceContainerLow

                Column {
                    id: inner

                    width: parent.width
                    spacing: 0

                    // Header.
                    Item {
                        width: parent.width
                        implicitHeight: header.implicitHeight + Tokens.padding.large * 2

                        Row {
                            id: header

                            anchors.left: parent.left
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.leftMargin: Tokens.padding.extraLargeIncreased
                            spacing: Tokens.spacing.medium

                            MaterialIcon {
                                anchors.verticalCenter: parent.verticalCenter
                                text: "build"
                                fill: 1
                                color: Colours.palette.m3primary
                                fontStyle: Tokens.font.icon.large

                                // Idle accent: slow breath while the menu is
                                // open and at least one mode is active.
                                SequentialAnimation on opacity {
                                    running: root.open && win.visible && Features.activeCount > 0
                                    loops: Animation.Infinite
                                    alwaysRunToEnd: true

                                    Anim {
                                        to: 0.8
                                        duration: 3000
                                        type: Anim.Standard
                                    }
                                    Anim {
                                        to: 1
                                        duration: 3000
                                        type: Anim.Standard
                                    }
                                }
                            }

                            Column {
                                anchors.verticalCenter: parent.verticalCenter

                                StyledText {
                                    text: qsTr("Features")
                                    color: Colours.palette.m3onSurface
                                    font: Tokens.font.body.builders.large.weight(Font.Bold).build()
                                }

                                StyledText {
                                    text: Features.activeCount > 0 ? qsTr("%1 active").arg(Features.activeCount) : qsTr("All off")
                                    color: Features.activeCount > 0 ? Colours.palette.m3primary : Colours.palette.m3onSurfaceVariant
                                    font: Tokens.font.body.small
                                }
                            }
                        }

                        MaterialIcon {
                            id: closeBtn

                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.rightMargin: Tokens.padding.extraLargeIncreased

                            text: "close"
                            color: closeLayer.containsMouse ? Colours.palette.m3primary : Colours.palette.m3onSurfaceVariant
                            scale: closeLayer.pressed ? 0.9 : 1

                            Behavior on scale {
                                Anim {
                                    type: closeLayer.pressed ? Anim.FastSpatial : Anim.Emphasized
                                }
                            }

                            StateLayer {
                                id: closeLayer
                                anchors.centerIn: parent
                                implicitWidth: parent.implicitHeight + Tokens.padding.medium
                                implicitHeight: implicitWidth
                                radius: Tokens.rounding.full
                                onClicked: Features.menuOpen = false
                            }
                        }
                    }

                    // One row per feature. Rows slide in staggered when the
                    // menu opens and squish slightly while pressed.
                    Repeater {
                        model: ScriptModel {
                            values: Features.features
                        }

                        Item {
                            id: row

                            required property var modelData
                            required property int index

                            width: inner.width
                            implicitHeight: rowContent.implicitHeight + Tokens.padding.large

                            opacity: root.open ? 1 : 0
                            scale: rowLayer.pressed ? 0.98 : 1

                            Behavior on opacity {
                                SequentialAnimation {
                                    PauseAnimation {
                                        duration: root.open ? 50 + row.index * 40 : 0
                                    }
                                    Anim {
                                        type: Anim.DefaultEffects
                                    }
                                }
                            }

                            Behavior on scale {
                                Anim {
                                    type: rowLayer.pressed ? Anim.FastSpatial : Anim.Emphasized
                                }
                            }

                            transform: Translate {
                                y: root.open ? 0 : -12

                                Behavior on y {
                                    SequentialAnimation {
                                        PauseAnimation {
                                            duration: root.open ? 50 + row.index * 40 : 0
                                        }
                                        Anim {
                                            type: Anim.DefaultSpatial
                                        }
                                    }
                                }
                            }

                            StateLayer {
                                id: rowLayer
                                radius: 0
                                onClicked: Features.toggle(row.modelData.id)
                            }

                            RowLayout {
                                id: rowContent

                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.leftMargin: Tokens.padding.extraLargeIncreased
                                anchors.rightMargin: Tokens.padding.extraLargeIncreased
                                spacing: Tokens.spacing.medium

                                MaterialIcon {
                                    text: row.modelData.icon
                                    color: row.modelData.enabled ? Colours.palette.m3primary : Colours.palette.m3onSurfaceVariant
                                    fill: row.modelData.enabled ? 1 : 0

                                    Behavior on fill {
                                        Anim {
                                            type: Anim.DefaultEffects
                                        }
                                    }
                                }

                                Column {
                                    Layout.fillWidth: true

                                    StyledText {
                                        text: row.modelData.name
                                        color: Colours.palette.m3onSurface
                                        font: Tokens.font.body.builders.medium.weight(Font.Medium).build()
                                    }

                                    StyledText {
                                        width: parent.width
                                        text: row.modelData.desc
                                        color: Colours.palette.m3onSurfaceVariant
                                        font: Tokens.font.body.small
                                        wrapMode: Text.WordWrap
                                    }
                                }

                                StyledSwitch {
                                    checked: row.modelData.enabled
                                    onToggled: Features.toggle(row.modelData.id)
                                }
                            }
                        }
                    }

                    // Footer hint.
                    StyledText {
                        width: parent.width - Tokens.padding.extraLargeIncreased * 2
                        x: Tokens.padding.extraLargeIncreased
                        topPadding: Tokens.padding.medium
                        bottomPadding: Tokens.padding.large
                        text: qsTr("Modes persist across reboots. Add more in services/Features.qml.")
                        color: Colours.palette.m3outline
                        font: Tokens.font.body.small
                        wrapMode: Text.WordWrap
                    }
                }
            }
        }
    }
}

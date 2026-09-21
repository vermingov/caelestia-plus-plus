pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import Caelestia.Config
import qs.components
import qs.components.containers
import qs.services

// Fullscreen overlay shown while an app is waiting on a verdict. Modal (grabs
// keyboard, dims the desktop) but clicking the scrim does NOT dismiss — a
// security prompt must be answered, not fat-fingered away.
Scope {
    id: root

    readonly property var current: Firewall.pending[0] ?? null
    // Not while the external bar asks: one question, asked once.
    readonly property bool open: Firewall.pendingCount > 0 && !ExternalBar.hasGuard
    readonly property var focusedScreen: Quickshell.screens.find(s => s.name === Hypr.focusedMonitor?.name) ?? Quickshell.screens[0]

    StyledWindow {
        id: win

        screen: root.focusedScreen
        name: "firewall-prompt"
        visible: root.open || closeTimer.running

        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.exclusionMode: ExclusionMode.Ignore
        WlrLayershell.keyboardFocus: root.open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

        anchors.top: true
        anchors.bottom: true
        anchors.left: true
        anchors.right: true

        // Holds the window alive through the exit animation.
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

        // Scrim.
        StyledRect {
            anchors.fill: parent
            color: Colours.palette.m3scrim
            opacity: root.open ? 0.55 : 0

            Behavior on opacity {
                Anim {
                    type: Anim.DefaultEffects
                }
            }

            MouseArea {
                anchors.fill: parent
            } // swallow clicks; intentionally no dismiss
        }

        // Card. Outer bezel keeps a primary-tinted hairline: this is
        // security UI and must read as such at a glance.
        StyledRect {
            id: card

            anchors.centerIn: parent
            implicitWidth: 460
            implicitHeight: content.implicitHeight + Tokens.padding.extraLargeIncreased * 2 + Tokens.padding.small * 2

            radius: Tokens.rounding.large
            color: Qt.alpha(Colours.palette.m3surfaceContainer, 0.85)
            border.width: 1
            border.color: Qt.alpha(Colours.palette.m3primary, 0.5)

            opacity: root.open ? 1 : 0
            scale: root.open ? 1 : 0.9

            transform: Translate {
                y: root.open ? 0 : 40

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

                radius: card.radius - Tokens.padding.small
                color: Colours.palette.m3surfaceContainerLow

                Column {
                    id: content

                    anchors.centerIn: parent
                    width: parent.width - Tokens.padding.extraLargeIncreased * 2
                    spacing: Tokens.spacing.medium

                    // Shield + heading.
                    Row {
                        width: parent.width
                        spacing: Tokens.spacing.medium

                        StyledRect {
                            id: badge

                            anchors.verticalCenter: parent.verticalCenter
                            implicitWidth: implicitHeight
                            implicitHeight: shield.implicitHeight + Tokens.padding.large
                            radius: Tokens.rounding.full
                            color: Qt.alpha(Colours.palette.m3primary, 0.15)

                            // Idle accent: the badge breathes slowly while a
                            // verdict is pending.
                            SequentialAnimation on scale {
                                running: root.open && win.visible
                                loops: Animation.Infinite
                                alwaysRunToEnd: true

                                Anim {
                                    to: 1.06
                                    duration: 2500
                                    type: Anim.Standard
                                }
                                Anim {
                                    to: 1
                                    duration: 2500
                                    type: Anim.Standard
                                }
                            }

                            MaterialIcon {
                                id: shield

                                anchors.centerIn: parent
                                text: "gpp_maybe"
                                fill: 1
                                color: Colours.palette.m3primary
                                fontStyle: Tokens.font.icon.large
                            }
                        }

                        Column {
                            anchors.verticalCenter: parent.verticalCenter
                            width: parent.width - parent.spacing - badge.width - Tokens.padding.large

                            StyledText {
                                text: qsTr("Connection request")
                                color: Colours.palette.m3onSurfaceVariant
                                font: Tokens.font.body.small
                            }

                            StyledText {
                                width: parent.width
                                text: root.current?.name ?? ""
                                color: Colours.palette.m3onSurface
                                font: Tokens.font.body.builders.large.weight(Font.Bold).build()
                                elide: Text.ElideRight
                            }
                        }
                    }

                    // Details.
                    StyledRect {
                        width: parent.width
                        implicitHeight: details.implicitHeight + Tokens.padding.large * 2
                        radius: Tokens.rounding.medium
                        color: Colours.palette.m3surfaceContainerHigh
                        border.width: 1
                        border.color: Qt.alpha(Colours.palette.m3outlineVariant, 0.3)

                        Column {
                            id: details

                            anchors.centerIn: parent
                            width: parent.width - Tokens.padding.large * 2
                            spacing: Tokens.spacing.small

                            DetailRow {
                                label: qsTr("Path")
                                value: root.current?.exe ?? ""
                                mono: true
                            }
                            DetailRow {
                                label: qsTr("Destination")
                                value: root.current ? `${root.current.dst}:${root.current.port}` : ""
                                mono: true
                            }
                            DetailRow {
                                label: qsTr("Protocol")
                                value: (root.current?.proto ?? "").toUpperCase()
                            }
                        }
                    }

                    StyledText {
                        visible: Firewall.pendingCount > 1
                        text: qsTr("＋%1 more waiting").arg(Firewall.pendingCount - 1)
                        color: Colours.palette.m3primary
                        font: Tokens.font.body.small
                    }

                    // Actions. Allow is the lone filled-primary action; Deny
                    // is an error-toned outline — the two verdicts can never
                    // be mistaken for each other. Once stays a quiet tonal.
                    Row {
                        width: parent.width
                        spacing: Tokens.spacing.small
                        layoutDirection: Qt.RightToLeft

                        PromptButton {
                            text: qsTr("Allow")
                            icon: "check"
                            accent: Colours.palette.m3primary
                            onColour: Colours.palette.m3onPrimary
                            onClicked: {
                                if (root.current)
                                    Firewall.allow(root.current.id);
                            }
                        }

                        PromptButton {
                            text: qsTr("Deny")
                            icon: "block"
                            outline: true
                            accent: Colours.palette.m3error
                            onColour: Colours.palette.m3onError
                            onClicked: {
                                if (root.current)
                                    Firewall.deny(root.current.id);
                            }
                        }

                        PromptButton {
                            text: qsTr("Once")
                            icon: "timelapse"
                            accent: Colours.palette.m3surfaceContainerHighest
                            onColour: Colours.palette.m3onSurface
                            onClicked: {
                                if (root.current)
                                    Firewall.allowOnce(root.current.id);
                            }
                        }
                    }
                }
            }
        }
    }

    component DetailRow: Row {
        required property string label
        property string value
        property bool mono: false

        width: parent.width
        spacing: Tokens.spacing.medium

        StyledText {
            width: 96
            text: parent.label
            color: Colours.palette.m3onSurfaceVariant
            font: Tokens.font.body.small
        }

        StyledText {
            width: parent.width - parent.spacing - 96
            text: parent.value
            color: Colours.palette.m3onSurface
            font: parent.mono ? Tokens.font.mono.small : Tokens.font.body.small
            elide: Text.ElideMiddle
        }
    }

    component PromptButton: StyledRect {
        id: btn

        required property string text
        required property string icon
        required property color accent
        required property color onColour
        property bool outline

        // Outline buttons draw in the accent itself over a transparent fill.
        readonly property color fg: outline ? accent : onColour

        signal clicked

        implicitWidth: row.implicitWidth + Tokens.padding.large * 2
        implicitHeight: row.implicitHeight + Tokens.padding.medium * 2
        radius: Tokens.rounding.full
        color: outline ? "transparent" : accent
        border.width: outline ? 1 : 0
        border.color: outline ? Qt.alpha(btn.accent, 0.7) : "transparent"
        scale: layer.pressed ? 0.96 : layer.containsMouse ? 1.04 : 1

        Behavior on scale {
            Anim {
                type: layer.pressed ? Anim.FastSpatial : Anim.Emphasized
            }
        }

        StateLayer {
            id: layer
            color: btn.fg
            onClicked: btn.clicked()
        }

        Row {
            id: row
            anchors.centerIn: parent
            spacing: Tokens.spacing.small

            MaterialIcon {
                anchors.verticalCenter: parent.verticalCenter
                text: btn.icon
                color: btn.fg
                fontStyle: Tokens.font.icon.small
            }

            StyledText {
                anchors.verticalCenter: parent.verticalCenter
                text: btn.text
                color: btn.fg
                font: Tokens.font.body.builders.medium.weight(Font.Medium).build()
            }
        }
    }
}

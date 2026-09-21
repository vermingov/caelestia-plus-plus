pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import Caelestia.Config
import qs.components
import qs.components.containers
import qs.services

// Freeze alert: shown while redguard holds a suspicious process (SIGSTOP)
// awaiting a verdict. Modal (grabs keyboard, dims the desktop); the scrim does
// NOT dismiss — a security prompt must be answered. Framed as a threat: Block
// is the prominent action, Allow is a deliberate outline, so the safe choice is
// the easy one.
Scope {
    id: root

    readonly property var current: Protection.pending[0] ?? null
    // Not while the external bar asks: one question, asked once.
    readonly property bool open: Protection.pendingCount > 0 && !ExternalBar.hasGuard
    readonly property var focusedScreen: Quickshell.screens.find(s => s.name === Hypr.focusedMonitor?.name) ?? Quickshell.screens[0]

    readonly property string kindLabel: {
        switch (root.current?.kind) {
        case "reverse-shell":
            return qsTr("Possible reverse shell");
        case "foreign-exec":
            return qsTr("Untrusted executable");
        default:
            return qsTr("Suspicious behavior");
        }
    }

    StyledWindow {
        id: win

        screen: root.focusedScreen
        name: "protection-prompt"
        visible: root.open || closeTimer.running

        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.exclusionMode: ExclusionMode.Ignore
        WlrLayershell.keyboardFocus: root.open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

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
            opacity: root.open ? 0.6 : 0

            Behavior on opacity {
                Anim {
                    type: Anim.DefaultEffects
                }
            }

            MouseArea {
                anchors.fill: parent
            }
        }

        StyledRect {
            id: card

            anchors.centerIn: parent
            implicitWidth: 480
            implicitHeight: content.implicitHeight + Tokens.padding.extraLargeIncreased * 2 + Tokens.padding.small * 2

            radius: Tokens.rounding.large
            color: Qt.alpha(Colours.palette.m3surfaceContainer, 0.9)
            border.width: 1
            border.color: Qt.alpha(Colours.palette.m3error, 0.6)

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
                anchors.fill: parent
                anchors.margins: Tokens.padding.small
                radius: card.radius - Tokens.padding.small
                color: Colours.palette.m3surfaceContainerLow

                Column {
                    id: content

                    anchors.centerIn: parent
                    width: parent.width - Tokens.padding.extraLargeIncreased * 2
                    spacing: Tokens.spacing.medium

                    Row {
                        width: parent.width
                        spacing: Tokens.spacing.medium

                        StyledRect {
                            id: badge
                            anchors.verticalCenter: parent.verticalCenter
                            implicitWidth: implicitHeight
                            implicitHeight: shield.implicitHeight + Tokens.padding.large
                            radius: Tokens.rounding.full
                            color: Qt.alpha(Colours.palette.m3error, 0.15)

                            SequentialAnimation on scale {
                                running: root.open && win.visible
                                loops: Animation.Infinite
                                alwaysRunToEnd: true

                                Anim {
                                    to: 1.08
                                    duration: 1200
                                    type: Anim.Standard
                                }
                                Anim {
                                    to: 1
                                    duration: 1200
                                    type: Anim.Standard
                                }
                            }

                            MaterialIcon {
                                id: shield
                                anchors.centerIn: parent
                                text: "gpp_bad"
                                fill: 1
                                color: Colours.palette.m3error
                                fontStyle: Tokens.font.icon.large
                            }
                        }

                        Column {
                            anchors.verticalCenter: parent.verticalCenter
                            width: parent.width - parent.spacing - badge.width - Tokens.padding.large

                            StyledText {
                                text: root.kindLabel + qsTr(" · frozen")
                                color: Colours.palette.m3error
                                font: Tokens.font.body.builders.small.weight(Font.Bold).build()
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

                    StyledText {
                        width: parent.width
                        text: root.current?.detail ?? ""
                        color: Colours.palette.m3onSurfaceVariant
                        font: Tokens.font.body.small
                        wrapMode: Text.WordWrap
                    }

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
                                label: qsTr("Spawned by")
                                value: root.current?.parent ?? "?"
                            }
                            DetailRow {
                                label: qsTr("PID")
                                value: `${root.current?.pid ?? ""}`
                                mono: true
                            }
                        }
                    }

                    StyledText {
                        visible: Protection.pendingCount > 1
                        text: qsTr("＋%1 more waiting").arg(Protection.pendingCount - 1)
                        color: Colours.palette.m3error
                        font: Tokens.font.body.small
                    }

                    // Block is the prominent action (this is a threat prompt);
                    // Allow is a deliberate outline so it is never fat-fingered.
                    Row {
                        width: parent.width
                        spacing: Tokens.spacing.small
                        layoutDirection: Qt.RightToLeft

                        PromptButton {
                            text: qsTr("Block")
                            icon: "dangerous"
                            accent: Colours.palette.m3error
                            onColour: Colours.palette.m3onError
                            onClicked: {
                                if (root.current)
                                    Protection.block(root.current.id);
                            }
                        }

                        PromptButton {
                            text: qsTr("Allow")
                            icon: "check"
                            outline: true
                            accent: Colours.palette.m3primary
                            onColour: Colours.palette.m3onPrimary
                            onClicked: {
                                if (root.current)
                                    Protection.allow(root.current.id);
                            }
                        }

                        PromptButton {
                            text: qsTr("Once")
                            icon: "timelapse"
                            accent: Colours.palette.m3surfaceContainerHighest
                            onColour: Colours.palette.m3onSurface
                            onClicked: {
                                if (root.current)
                                    Protection.allowOnce(root.current.id);
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

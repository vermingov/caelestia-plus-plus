import QtQuick
import QtQuick.Layouts
import Caelestia.Config
import qs.components
import qs.services
import qs.modules.nexus.common

PageBase {
    id: root

    title: qsTr("Compositor")

    ColumnLayout {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        width: root.cappedWidth
        spacing: Tokens.spacing.extraSmall / 2

        ColumnLayout {
            visible: !HyprMod.available
            Layout.fillWidth: true
            Layout.topMargin: Tokens.padding.extraLarge
            spacing: Tokens.padding.extraSmall

            MaterialIcon {
                Layout.alignment: Qt.AlignHCenter
                text: "warning"
                color: Colours.palette.m3outlineVariant
                fontStyle: Tokens.font.icon.extraLarge
            }

            StyledText {
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("HyprMod not detected")
                color: Colours.palette.m3outlineVariant
                font: Tokens.font.title.large
            }

            StyledText {
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("These settings need the HyprMod Hyprland customizer and its variables.lua.")
                color: Colours.palette.m3outlineVariant
                font: Tokens.font.body.large
            }
        }

        NavRow {
            visible: HyprMod.available
            first: true
            icon: "blur_on"
            label: qsTr("Appearance")
            status: HyprMod.get("blurEnabled", false) ? qsTr("Blur on") : qsTr("Blur off")
            onClicked: root.nState.openSubPage(1)
        }

        NavRow {
            visible: HyprMod.available
            icon: "space_dashboard"
            label: qsTr("Layout & gaps")
            status: qsTr("Gaps %1 / %2").arg(HyprMod.get("windowGapsIn", 0)).arg(HyprMod.get("windowGapsOut", 0))
            onClicked: root.nState.openSubPage(2)
        }

        NavRow {
            visible: HyprMod.available
            icon: "touchpad_mouse"
            label: qsTr("Input & gestures")
            status: qsTr("%1-finger gestures").arg(HyprMod.get("gestureFingers", 3))
            onClicked: root.nState.openSubPage(3)
        }

        NavRow {
            visible: HyprMod.available
            icon: "apps"
            label: qsTr("Default apps")
            status: HyprMod.get("terminal", "")
            onClicked: root.nState.openSubPage(4)
        }

        NavRow {
            visible: HyprMod.available
            icon: "keyboard"
            label: qsTr("Keybinds")
            status: qsTr("Workspaces, windows, apps")
            onClicked: root.nState.openSubPage(5)
        }

        NavRow {
            visible: HyprMod.available
            icon: "tune"
            label: qsTr("System")
            status: qsTr("Volume, cursor, sleep")
            onClicked: root.nState.openSubPage(6)
        }

        NavRow {
            visible: HyprMod.available
            icon: "monitor"
            label: qsTr("Monitors")
            status: Hypr.monitors.values.length === 1 ? qsTr("1 monitor") : qsTr("%1 monitors").arg(Hypr.monitors.values.length)
            onClicked: root.nState.openSubPage(8)
        }

        NavRow {
            visible: HyprMod.available
            icon: "memory"
            label: qsTr("App GPUs")
            status: qsTr("Per-app GPU selection")
            onClicked: root.nState.openSubPage(9)
        }

        NavRow {
            visible: HyprMod.available
            last: true
            icon: "manufacturing"
            label: qsTr("All options")
            status: qsTr("Every compositor option, searchable")
            onClicked: root.nState.openSubPage(7)
        }
    }
}

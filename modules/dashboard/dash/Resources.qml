import QtQuick
import QtQuick.Layouts
import Caelestia.Config
import Caelestia.Services
import qs.components
import qs.components.controls
import qs.services

Item {
    id: root

    anchors.top: parent.top
    anchors.bottom: parent.bottom

    implicitWidth: layout.implicitWidth + layout.anchors.margins * 2

    // Resident content: hold the poller refs only while on screen. Unseen
    // percentage updates fire Behavior animations, and any running animation
    // forces this whole window to render at 60fps even when invisible.
    Loader {
        active: root.visible

        sourceComponent: Item {
            ServiceRef {
                service: Cpu
            }

            ServiceRef {
                service: Memory
            }

            ServiceRef {
                service: Storage
            }
        }
    }

    ColumnLayout {
        id: layout

        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.margins: Tokens.padding.large
        spacing: Tokens.spacing.medium

        Resource {
            icon: "memory"
            value: Cpu.percentage
        }

        Resource {
            icon: "memory_alt"
            value: Memory.percentage
            fgColour: Colours.palette.m3tertiary
        }

        Resource {
            icon: "hard_disk"
            value: Storage.percentage
            fgColour: Colours.palette.m3secondary
        }
    }
    component Resource: CircularProgress {
        id: res

        required property string icon

        Layout.fillHeight: true
        implicitSize: height
        strokeWidth: Tokens.sizes.dashboard.resourceProgressThickness

        // Resident while the dashboard is closed: an animation nobody can
        // see still renders this whole window at 60fps for its full half
        // second, once per reading. Off screen the value just jumps.
        Behavior on clampedVal {
            enabled: res.visible

            Anim {}
        }

        MaterialIcon {
            anchors.centerIn: parent
            text: res.icon
            font: Tokens.font.icon.large
            color: res.fgColour
        }
    }
}

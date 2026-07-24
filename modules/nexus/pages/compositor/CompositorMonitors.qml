pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Quickshell
import Caelestia.Config
import qs.components
import qs.components.controls
import qs.services
import qs.modules.nexus.common

PageBase {
    id: root

    // Live layout from the compositor: logical geometry per output. Saved
    // overrides are applied through hl.monitor eval, so IPC state is the
    // single source of truth the canvas renders.
    readonly property var liveMonitors: Hypr.monitors.values.map(m => {
        const ipc = m.lastIpcObject;
        const swap = ipc.transform % 2 === 1;
        return {
            name: ipc.name,
            x: ipc.x,
            y: ipc.y,
            w: Math.round((swap ? ipc.height : ipc.width) / ipc.scale),
            h: Math.round((swap ? ipc.width : ipc.height) / ipc.scale),
            mode: `${ipc.width}x${ipc.height}@${Math.round(ipc.refreshRate)}`,
            scale: ipc.scale,
            disabled: ipc.disabled,
            availableModes: ipc.availableModes
        };
    })
    property string selected: liveMonitors[0]?.name ?? ""
    readonly property var selectedMonitor: liveMonitors.find(m => m.name === root.selected) ?? null

    // Current saved spec for a monitor, seeded from live state so a single
    // edit (say scale) doesn't reset the fields the user never touched.
    function specFor(name: string): var {
        const live = liveMonitors.find(m => m.name === name);
        const saved = HyprMod.monitors[name] ?? {};
        return {
            mode: saved.mode ?? live.mode,
            position: saved.position ?? `${live.x}x${live.y}`,
            scale: saved.scale ?? live.scale
        };
    }

    function update(name: string, changes: var): void {
        HyprMod.setMonitor(name, Object.assign(specFor(name), changes));
    }

    title: qsTr("Monitors")
    isSubPage: true

    Component.onCompleted: HyprMod.refreshState()

    ColumnLayout {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        width: root.cappedWidth
        spacing: Tokens.spacing.extraSmall / 2

        Variants {
            id: modeItems

            model: root.selectedMonitor?.availableModes ?? []

            MenuItem {
                required property string modelData

                text: modelData.replace(/\.00Hz$/, "").replace(/Hz$/, "")
            }
        }

        SectionHeader {
            first: true
            text: qsTr("Drag monitors to arrange them — changes apply immediately")
        }

        ConnectedRect {
            Layout.fillWidth: true
            first: true
            last: true
            implicitHeight: 260

            MonitorCanvas {
                anchors.fill: parent
                anchors.margins: Tokens.padding.medium
                monitors: root.liveMonitors
                selected: root.selected
                primaryName: HyprMod.primaryMonitor
                onMonitorSelected: name => root.selected = name
                onMonitorMoved: (name, x, y) => root.update(name, {
                    position: `${x}x${y}`
                })
            }
        }

        SectionHeader {
            visible: root.selectedMonitor !== null
            text: root.selectedMonitor ? qsTr("Settings for %1").arg(root.selectedMonitor.name) : ""
        }

        SelectRow {
            visible: root.selectedMonitor !== null
            first: true
            label: qsTr("Resolution & refresh rate")
            menuItems: modeItems.instances
            active: menuItems.find(i => i.text === root.selectedMonitor?.mode) ?? null
            fallbackText: root.selectedMonitor?.mode ?? ""
            onSelected: item => root.update(root.selected, {
                mode: item.text
            })
        }

        StepperRow {
            visible: root.selectedMonitor !== null
            label: qsTr("Scale")
            subtext: qsTr("Interface size on this monitor")
            value: root.selectedMonitor?.scale ?? 1
            from: 0.5
            to: 3
            stepSize: 0.25
            onMoved: v => root.update(root.selected, {
                scale: v
            })
        }

        ToggleRow {
            visible: root.selectedMonitor !== null
            text: qsTr("Primary monitor")
            subtext: qsTr("Workspace 1 lives here after restarts")
            checked: HyprMod.primaryMonitor === root.selected
            onToggled: HyprMod.setPrimary(checked ? root.selected : "")
        }

        ConnectedRect {
            visible: root.selectedMonitor !== null
            Layout.fillWidth: true
            last: true
            implicitHeight: resetButton.implicitHeight + Tokens.padding.medium * 2

            IconTextButton {
                id: resetButton

                anchors.centerIn: parent
                icon: "restart_alt"
                text: qsTr("Forget saved layout")
                font: Tokens.font.body.large
                isRound: true
                type: IconTextButton.Tonal
                disabled: HyprMod.monitors[root.selected] === undefined
                onClicked: HyprMod.delMonitor(root.selected)
            }
        }
    }
}

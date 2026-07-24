pragma ComponentBehavior: Bound

import QtQuick
import Caelestia.Config
import qs.components
import qs.services

// Windows-style monitor arrangement canvas. Each output is a draggable
// rectangle scaled to fit; coordinates are logical pixels (size / scale).
// Drops snap to neighbouring edges so layouts stay gapless.
Item {
    id: root

    // [{name, x, y, w, h, mode}] logical geometry per monitor
    required property var monitors
    property string selected
    property string primaryName

    signal monitorMoved(name: string, x: int, y: int)
    signal monitorSelected(name: string)

    readonly property real pad: Tokens.padding.large
    readonly property var bounds: {
        let minX = 0, minY = 0, maxX = 1, maxY = 1;
        for (const m of monitors) {
            minX = Math.min(minX, m.x);
            minY = Math.min(minY, m.y);
            maxX = Math.max(maxX, m.x + m.w);
            maxY = Math.max(maxY, m.y + m.h);
        }
        return {
            x: minX,
            y: minY,
            w: maxX - minX,
            h: maxY - minY
        };
    }
    readonly property real canvasScale: Math.min((width - pad * 2) / bounds.w, (height - pad * 2) / bounds.h)
    readonly property real originX: (width - bounds.w * canvasScale) / 2 - bounds.x * canvasScale
    readonly property real originY: (height - bounds.h * canvasScale) / 2 - bounds.y * canvasScale

    function snapped(name: string, lx: real, ly: real): var {
        const self = monitors.find(m => m.name === name);
        const threshold = 40 / canvasScale;
        let bestX = Math.round(lx), bestY = Math.round(ly);
        let dx = threshold, dy = threshold;
        const candX = [0], candY = [0];
        for (const o of monitors) {
            if (o.name === name)
                continue;
            candX.push(o.x + o.w, o.x - self.w, o.x, o.x + o.w - self.w);
            candY.push(o.y + o.h, o.y - self.h, o.y, o.y + o.h - self.h);
        }
        for (const c of candX)
            if (Math.abs(lx - c) < dx) {
                dx = Math.abs(lx - c);
                bestX = c;
            }
        for (const c of candY)
            if (Math.abs(ly - c) < dy) {
                dy = Math.abs(ly - c);
                bestY = c;
            }
        return {
            x: bestX,
            y: bestY
        };
    }

    Repeater {
        model: root.monitors

        Rectangle {
            id: monRect

            required property var modelData
            readonly property bool isSelected: root.selected === modelData.name

            width: modelData.w * root.canvasScale
            height: modelData.h * root.canvasScale
            radius: Tokens.rounding.small
            color: isSelected ? Colours.palette.m3primaryContainer : Colours.palette.m3surfaceContainerHighest
            border.width: isSelected ? 2 : 1
            border.color: isSelected ? Colours.palette.m3primary : Colours.palette.m3outlineVariant

            Binding on x {
                when: !dragArea.drag.active
                value: monRect.modelData.x * root.canvasScale + root.originX
                restoreMode: Binding.RestoreNone
            }

            Binding on y {
                when: !dragArea.drag.active
                value: monRect.modelData.y * root.canvasScale + root.originY
                restoreMode: Binding.RestoreNone
            }

            MouseArea {
                id: dragArea

                anchors.fill: parent
                drag.target: monRect
                preventStealing: true
                cursorShape: drag.active ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                onPressed: root.monitorSelected(monRect.modelData.name)
                onReleased: {
                    const pos = root.snapped(monRect.modelData.name, (monRect.x - root.originX) / root.canvasScale, (monRect.y - root.originY) / root.canvasScale);
                    root.monitorMoved(monRect.modelData.name, pos.x, pos.y);
                }
            }

            Column {
                anchors.centerIn: parent
                spacing: 0

                StyledText {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: (root.primaryName === monRect.modelData.name ? "★ " : "") + monRect.modelData.name
                    color: monRect.isSelected ? Colours.palette.m3onPrimaryContainer : Colours.palette.m3onSurface
                    font: Tokens.font.body.small
                }

                StyledText {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: monRect.modelData.mode
                    color: monRect.isSelected ? Colours.palette.m3onPrimaryContainer : Colours.palette.m3outline
                    font: Tokens.font.label.small
                }
            }
        }
    }
}

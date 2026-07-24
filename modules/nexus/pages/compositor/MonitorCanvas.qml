pragma ComponentBehavior: Bound

import QtQuick
import Caelestia.Config
import qs.components
import qs.services

// Monitor arrangement canvas. Drag behaviour matches hyprmod's layout
// preview: the monitor follows the cursor live in logical coordinates on a
// 10px grid, overlaps push the dragged output out along its approach axis
// so layouts stay gapless, and the total extent is clamped so nothing can
// be flung out of reach. The move is committed once, on release.
Item {
    id: root

    // [{name, x, y, w, h, mode, disabled}] logical geometry per monitor
    required property var monitors
    property string selected
    property string primaryName

    signal monitorMoved(name: string, x: int, y: int)
    signal monitorSelected(name: string)

    // Rendered copy of `monitors`, refreshed only between drags: live IPC
    // updates mid-gesture would otherwise rebuild the repeater and destroy
    // the pressed delegate. Freezing it also keeps the canvas transform
    // stable for the whole drag.
    property var layout: monitors
    property string dragging
    property int dragX
    property int dragY

    onMonitorsChanged: {
        if (!dragging)
            layout = monitors;
    }

    readonly property real pad: Tokens.padding.large
    readonly property var bounds: {
        let minX = 0, minY = 0, maxX = 1, maxY = 1;
        for (const m of layout) {
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

    // hyprmod's collision resolver: on each axis that was separated at drag
    // start, a centre-vs-centre comparison picks the push side (symmetric
    // crossing "pressure" without cross-axis oscillation); of the valid
    // pushes, the shortest wins. Only the dragged monitor ever moves.
    function resolveCollisions(self, x, y, startX, startY): var {
        for (const o of layout) {
            if (o.name === self.name)
                continue;
            if (x >= o.x + o.w || x + self.w <= o.x || y >= o.y + o.h || y + self.h <= o.y)
                continue;

            const hSep = startX + self.w <= o.x || startX >= o.x + o.w;
            const vSep = startY + self.h <= o.y || startY >= o.y + o.h;
            const pushes = [];
            if (hSep)
                pushes.push(x + self.w / 2 < o.x + o.w / 2 ? {
                    x: o.x - self.w,
                    y,
                    dist: x + self.w - o.x
                } : {
                    x: o.x + o.w,
                    y,
                    dist: o.x + o.w - x
                });
            if (vSep)
                pushes.push(y + self.h / 2 < o.y + o.h / 2 ? {
                    x,
                    y: o.y - self.h,
                    dist: y + self.h - o.y
                } : {
                    x,
                    y: o.y + o.h,
                    dist: o.y + o.h - y
                });

            if (pushes.length === 0) {
                // Started already overlapping — push along the axis with
                // more relative displacement.
                if (Math.abs(x + self.w / 2 - o.x - o.w / 2) * (self.h + o.h) >= Math.abs(y + self.h / 2 - o.y - o.h / 2) * (self.w + o.w))
                    x = x + self.w / 2 < o.x + o.w / 2 ? o.x - self.w : o.x + o.w;
                else
                    y = y + self.h / 2 < o.y + o.h / 2 ? o.y - self.h : o.y + o.h;
                continue;
            }

            const best = pushes.reduce((a, b) => b.dist < a.dist ? b : a);
            x = best.x;
            y = best.y;
        }
        return {
            x,
            y
        };
    }

    // Keep the combined bounding box within 3x the summed monitor sizes so
    // a monitor can't be dragged far enough to vanish from the canvas.
    function clampToNeighbors(self, x, y): var {
        const active = layout.filter(m => !m.disabled);
        if (active.length < 2)
            return {
                x,
                y
            };
        const maxW = active.reduce((sum, m) => sum + m.w, 0) * 3;
        const maxH = active.reduce((sum, m) => sum + m.h, 0) * 3;
        const others = active.filter(m => m.name !== self.name);
        const minOx = Math.min(...others.map(m => m.x));
        const minOy = Math.min(...others.map(m => m.y));
        const maxOx = Math.max(...others.map(m => m.x + m.w));
        const maxOy = Math.max(...others.map(m => m.y + m.h));
        return {
            x: Math.min(Math.max(x, Math.min(minOx, maxOx - maxW)), Math.max(maxOx, minOx + maxW) - self.w),
            y: Math.min(Math.max(y, Math.min(minOy, maxOy - maxH)), Math.max(maxOy, minOy + maxH) - self.h)
        };
    }

    Repeater {
        model: root.layout

        Rectangle {
            id: monRect

            required property var modelData
            readonly property bool isSelected: root.selected === modelData.name
            readonly property bool isDragged: root.dragging === modelData.name

            x: (isDragged ? root.dragX : modelData.x) * root.canvasScale + root.originX
            y: (isDragged ? root.dragY : modelData.y) * root.canvasScale + root.originY
            width: modelData.w * root.canvasScale
            height: modelData.h * root.canvasScale
            radius: Tokens.rounding.small
            color: isSelected ? Colours.palette.m3primaryContainer : Colours.palette.m3surfaceContainerHighest
            border.width: isSelected ? 2 : 1
            border.color: isSelected ? Colours.palette.m3primary : Colours.palette.m3outlineVariant
            opacity: modelData.disabled ? 0.4 : 1

            MouseArea {
                id: dragArea

                property real pressX
                property real pressY
                property int startX
                property int startY
                property bool moved

                anchors.fill: parent
                enabled: !monRect.modelData.disabled
                preventStealing: true
                cursorShape: monRect.isDragged ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                onPressed: mouse => {
                    root.monitorSelected(monRect.modelData.name);
                    const p = mapToItem(root, mouse.x, mouse.y);
                    pressX = p.x;
                    pressY = p.y;
                    startX = monRect.modelData.x;
                    startY = monRect.modelData.y;
                    moved = false;
                    root.dragX = startX;
                    root.dragY = startY;
                    root.dragging = monRect.modelData.name;
                }
                onPositionChanged: mouse => {
                    if (!monRect.isDragged)
                        return;
                    const p = mapToItem(root, mouse.x, mouse.y);
                    const gx = Math.round((startX + (p.x - pressX) / root.canvasScale) / 10) * 10;
                    const gy = Math.round((startY + (p.y - pressY) / root.canvasScale) / 10) * 10;
                    let pos = root.resolveCollisions(monRect.modelData, gx, gy, startX, startY);
                    pos = root.clampToNeighbors(monRect.modelData, pos.x, pos.y);
                    if (pos.x !== root.dragX || pos.y !== root.dragY) {
                        root.dragX = pos.x;
                        root.dragY = pos.y;
                        moved = true;
                    }
                }
                onReleased: {
                    const name = monRect.modelData.name;
                    const x = root.dragX, y = root.dragY;
                    const commit = moved;
                    root.dragging = "";
                    if (commit) {
                        // Show the drop position until the compositor's IPC
                        // state catches up and replaces the layout.
                        root.layout = root.layout.map(m => m.name === name ? Object.assign({}, m, {
                            x,
                            y
                        }) : m);
                        root.monitorMoved(name, x, y);
                    }
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

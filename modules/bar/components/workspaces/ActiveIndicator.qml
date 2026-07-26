pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Effects
import Caelestia.Config
import qs.components
import qs.components.effects
import qs.services

StyledRect {
    id: root

    required property int activeWsId
    required property Repeater workspaces
    required property Item mask
    required property bool fullscreen

    property real glowStrength: 0.4

    // Euclidean modulo, so special workspaces (negative ids) wrap into range.
    // `shown` is clamped because a configured 0 would otherwise make the old
    // `while (i < 0) i += shown` loop spin forever and hard-lock the shell.
    readonly property int currentWsIdx: {
        const shown = Math.max(1, Config.bar.workspaces.shown);
        return ((activeWsId - 1) % shown + shown) % shown;
    }

    property real leading: workspaces.count > 0 ? workspaces.itemAt(currentWsIdx)?.x ?? 0 : 0
    property real trailing: workspaces.count > 0 ? workspaces.itemAt(currentWsIdx)?.x ?? 0 : 0
    property real currentSize: workspaces.count > 0 ? (workspaces.itemAt(currentWsIdx) as Workspace)?.size ?? 0 : 0
    property real offset: Math.min(leading, trailing)
    property real size: {
        const s = Math.abs(leading - trailing) + currentSize;
        if (Config.bar.workspaces.activeTrail && lastWs > currentWsIdx) {
            const ws = workspaces.itemAt(lastWs) as Workspace;
            return ws ? Math.min(ws.x + ws.size - offset, s) : 0;
        }
        return s;
    }

    property int cWs
    property int lastWs

    onCurrentWsIdxChanged: {
        lastWs = cWs;
        cWs = currentWsIdx;
        if (visible && !fullscreen)
            glowBreath.restart();
    }

    x: offset + mask.x
    implicitHeight: Tokens.sizes.bar.innerWidth - Tokens.padding.small
    implicitWidth: size
    radius: Tokens.rounding.full
    color: Colours.palette.m3primary

    RectangularShadow {
        z: -1
        anchors.fill: parent
        radius: parent.radius
        color: Qt.alpha(Colours.palette.m3primary, root.glowStrength)
        blur: Tokens.padding.large
        spread: 1
        offset: Qt.vector2d(0, 0)
    }

    // Glow breathes for two cycles after a workspace switch, then rests.
    // No `running:` binding: a binding on a finite animation re-asserts true
    // after completion and restarts it forever — the always-on loop it
    // replaces cost ~4% CPU in constant 60fps bar re-renders.
    SequentialAnimation on glowStrength {
        id: glowBreath

        loops: 2
        alwaysRunToEnd: true

        Anim {
            to: 0.9
            duration: Tokens.anim.durations.extraLarge * 3
        }
        Anim {
            to: 0.4
            duration: Tokens.anim.durations.extraLarge * 3
        }
    }

    Item {
        anchors.fill: parent
        clip: true

        Colouriser {
            source: root.mask
            sourceColor: Colours.palette.m3onSurface
            colorizationColor: Colours.palette.m3onPrimary

            x: -root.offset
            implicitWidth: root.mask.implicitWidth
            implicitHeight: root.mask.implicitHeight

            anchors.verticalCenter: parent.verticalCenter
        }
    }

    Behavior on leading {
        enabled: root.Config.bar.workspaces.activeTrail

        EAnim {}
    }

    Behavior on trailing {
        enabled: root.Config.bar.workspaces.activeTrail

        EAnim {
            duration: Tokens.anim.durations.normal * 2
        }
    }

    Behavior on currentSize {
        enabled: root.Config.bar.workspaces.activeTrail

        EAnim {}
    }

    Behavior on offset {
        enabled: !root.Config.bar.workspaces.activeTrail

        EAnim {}
    }

    Behavior on size {
        enabled: !root.Config.bar.workspaces.activeTrail

        EAnim {}
    }

    component EAnim: Anim {
        type: Anim.Emphasized
    }
}

pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Caelestia.Config
import qs.components
import qs.modules.launcher.services
// Namespaced: this module has a `services` of its own, and the singleton
// wanted here is the shell's.
import qs.services as Shell

Item {
    id: root

    required property ShellScreen screen
    required property ScreenState screenState
    required property var panels

    // Never opens while the external launcher is installed. The shortcut
    // routes there already, but `showall`, the `drawers toggle launcher` IPC
    // and anything else that sets the screen state directly do not know about
    // it — and two launchers answering one keypress is how the QML one kept
    // appearing. Everything below is left intact; uninstalling the other one
    // puts this back.
    readonly property bool shouldBeActive: screenState.launcher && Config.launcher.enabled && !Shell.Launcher.external

    readonly property real maxHeight: {
        let max = screen.height * 0.62;
        if (screenState.dashboard)
            max -= panels.dashboard.nonAnimHeight;
        return max;
    }

    property real offsetScale: shouldBeActive ? 0 : 1

    onShouldBeActiveChanged: {
        if (shouldBeActive)
            implicitHeight = Qt.binding(() => content.implicitHeight);
        else
            implicitHeight = implicitHeight; // Break binding during close anim
    }

    visible: offsetScale < 1
    implicitHeight: content.implicitHeight
    implicitWidth: Style.panelWidth
    opacity: 1 - offsetScale

    // Shrinks as it fades rather than vanishing at full size. This is not
    // decoration: the compositor cannot fade a blur, so a panel that fades
    // out at full size keeps its blurred backdrop until alpha hits zero and
    // then snaps the whole region back to sharp in one frame. Shrinking
    // means the blurred region closes to almost nothing first, so there is
    // no large area left to snap. Text is NativeRendering and does go soft
    // under a fractional scale, which is fine on the way out and is why the
    // scale is kept small on the way in.
    transform: [
        Scale {
            // Collapses hard on the way out, barely moves on the way in. The
            // blur region is whatever still has alpha, so the smaller the
            // panel is when alpha finally hits zero, the less area there is
            // to snap back to sharp. Entry stays near 1 because scaling
            // NativeRendering text softens it.
            readonly property real amount: root.shouldBeActive ? 0.04 : 0.5

            origin.x: root.width / 2
            origin.y: root.height / 2
            xScale: 1 - root.offsetScale * amount
            yScale: 1 - root.offsetScale * amount
        },
        Translate {
            y: root.offsetScale * 16
        }
    ]

    Component.onCompleted: Qt.callLater(() => Apps) // Load apps on init

    // Close eases out rather than snapping: FastEffects front-loads too
    // little of the fade, so the panel sat there and then vanished
    Behavior on offsetScale {
        Anim {
            type: root.shouldBeActive ? Anim.DefaultSpatial : Anim.DefaultEffects
            easing: root.shouldBeActive ? Tokens.anim.expressiveDefaultSpatial : Tokens.anim.standardDecel
        }
    }

    Loader {
        id: content

        anchors.top: parent.top
        anchors.horizontalCenter: parent.horizontalCenter

        // Preloaded at startup and kept resident: sync-instantiating this
        // tree on open froze the GUI thread mid-entrance (visible jank).
        // Loaded synchronously at startup: async incubation raced service
        // threads ("Cannot create children for a parent in a different
        // thread") and silently aborted, leaving the panel empty.
        active: true

        sourceComponent: Content {
            screenState: root.screenState
            panels: root.panels
            maxHeight: root.maxHeight
            openProgress: 1 - root.offsetScale
        }
    }
}

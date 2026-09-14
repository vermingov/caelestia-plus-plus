pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Caelestia.Config
import qs.components
import qs.modules.launcher.services

Item {
    id: root

    required property ShellScreen screen
    required property ScreenState screenState
    required property var panels

    readonly property bool shouldBeActive: screenState.launcher && Config.launcher.enabled

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

    // Rises the last few pixels into place instead of sliding in from an
    // edge. Deliberately no scale on the way in: the panel's text renders
    // with NativeRendering, which blurs under a fractional scale
    // (see Bible qml-nativerendering-blurs-under-scale).
    transform: Translate {
        y: root.offsetScale * 16
    }

    Component.onCompleted: Qt.callLater(() => Apps) // Load apps on init

    Behavior on offsetScale {
        Anim {
            type: root.shouldBeActive ? Anim.DefaultSpatial : Anim.FastEffects
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

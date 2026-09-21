pragma ComponentBehavior: Bound

import QtQuick
import Caelestia.Config
import qs.components
import qs.services

Item {
    id: root

    required property ScreenState screenState
    required property bool sidebarVisible
    readonly property real nonAnimWidth: content.implicitWidth

    // Not while cae draws the session menu.
    readonly property bool shouldBeActive: screenState.session && Config.session.enabled && !ExternalBar.hasSession
    property real offsetScale: shouldBeActive ? 0 : 1
    property real sidebarOffset: sidebarVisible ? 14 : 0

    visible: offsetScale < 1
    anchors.rightMargin: (-implicitWidth - 5 - sidebarOffset) * offsetScale
    implicitWidth: content.implicitWidth
    implicitHeight: content.implicitHeight || 510 // Hard coded fallback for first open
    opacity: 1 - offsetScale

    Behavior on offsetScale {
        Anim {}
    }

    Loader {
        id: content

        anchors.verticalCenter: parent.verticalCenter
        anchors.left: parent.left

        // Preloaded at startup and kept resident: sync-instantiating this
        // tree on open froze the GUI thread mid-entrance (visible jank).
        // Loaded synchronously at startup: async incubation raced service
        // threads ("Cannot create children for a parent in a different
        // thread") and silently aborted, leaving the panel empty.
        active: true


        sourceComponent: Content {
            screenState: root.screenState
        }
    }
}

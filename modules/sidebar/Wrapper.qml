pragma ComponentBehavior: Bound

import QtQuick
import Caelestia
import Caelestia.Config
import qs.components
import qs.services

Item {
    id: root

    required property ScreenState screenState
    readonly property Props props: Props {}

    // Not while the external bar draws its own notification centre: the two
    // are the same list, out of the same edge.
    readonly property bool shouldBeActive: screenState.sidebar && Config.sidebar.enabled && !Notifs.external
    property real offsetScale: shouldBeActive ? 0 : 1

    visible: offsetScale < 1
    anchors.rightMargin: (-implicitWidth - 5) * offsetScale
    implicitWidth: Tokens.sizes.sidebar.width
    opacity: 1 - offsetScale

    Behavior on offsetScale {
        Anim {}
    }

    Loader {
        id: content

        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.leftMargin: Tokens.padding.large
        anchors.margins: CUtils.clamp(anchors.leftMargin - Config.border.thickness, 0, anchors.leftMargin)
        anchors.bottomMargin: 0

        // Preloaded at startup and kept resident: sync-instantiating this
        // tree on open froze the GUI thread mid-entrance (visible jank).
        // Loaded synchronously at startup: async incubation raced service
        // threads ("Cannot create children for a parent in a different
        // thread") and silently aborted, leaving the panel empty.
        active: true


        sourceComponent: Content {
            implicitWidth: Tokens.sizes.sidebar.width - content.anchors.leftMargin - content.anchors.margins
            props: root.props
            screenState: root.screenState
        }
    }
}

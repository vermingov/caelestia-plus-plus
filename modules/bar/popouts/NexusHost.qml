import QtQuick
import Quickshell
import Caelestia.Config
import qs.components
import qs.modules.nexus

// The settings panel behind a URL loader (see Wrapper.qml): keeps the nexus
// module, the largest tree in the shell, out of the startup compile.
StyledClippingRect {
    id: root

    property ShellScreen screen
    property bool animating
    property int pageIdx: -1

    signal close

    radius: Tokens.rounding.extraLarge
    implicitWidth: nexus.implicitWidth
    implicitHeight: nexus.implicitHeight

    Nexus {
        id: nexus

        anchors.fill: parent
        nState.screen: root.screen
        nState.animatingContainer: root.animating
        nState.currentPageIdx: root.pageIdx
        onClose: root.close()
    }
}

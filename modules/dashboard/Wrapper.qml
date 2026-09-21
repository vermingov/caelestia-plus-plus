pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Caelestia
import Caelestia.Config
import qs.components
import qs.components.filedialog
import qs.services
import qs.utils

Item {
    id: root

    required property ScreenState screenState
    readonly property FileDialog facePicker: FileDialog {
        title: qsTr("Select a profile picture")
        filterLabel: qsTr("Image files")
        filters: Images.validImageExtensions
        onAccepted: path => {
            if (CUtils.copyFile(Qt.resolvedUrl(path), Qt.resolvedUrl(`${Paths.home}/.face`)))
                Quickshell.execDetached(["notify-send", "-a", "caelestia++", "-u", "low", "-h", `STRING:image-path:${path}`, "Profile picture changed", `Profile picture changed to ${Paths.shortenHome(path)}`]);
            else
                Quickshell.execDetached(["notify-send", "-a", "caelestia++", "-u", "critical", "Unable to change profile picture", `Failed to change profile picture to ${Paths.shortenHome(path)}`]);
        }
    }

    // Compact corner panel: the full dashboard scaled down into the
    // bottom-left, mirroring the utilities quick menu on the right
    readonly property real compactScale: 0.8
    readonly property real nonAnimHeight: ((content.item as Content)?.nonAnimHeight ?? 0) * compactScale
    // Not while cae draws the dashboard: it rises out of the same corner.
    readonly property bool shouldBeActive: screenState.dashboard && Config.dashboard.enabled && !ExternalBar.hasDashboard
    property real offsetScale: shouldBeActive ? 0 : 1

    visible: offsetScale < 1
    anchors.bottomMargin: (-implicitHeight - 5) * offsetScale
    implicitHeight: content.implicitHeight * compactScale
    implicitWidth: (content.implicitWidth || 854) * compactScale // Hard coded fallback for first open
    opacity: 1 - offsetScale

    Behavior on offsetScale {
        Anim {}
    }

    // Panel size only changes when a larger tab loads for the first time;
    // morph there instead of snapping
    Behavior on implicitWidth {
        Anim {}
    }

    Behavior on implicitHeight {
        Anim {}
    }

    Loader {
        id: content

        // Layout geometry stays unscaled; the visual shrinks around the
        // bottom-left corner to exactly fill the wrapper's scaled size
        anchors.left: parent.left
        anchors.bottom: parent.bottom
        scale: root.compactScale
        transformOrigin: Item.BottomLeft
        // Natively-rendered glyphs go fuzzy under a plain transform scale;
        // rasterising the tree at full size and letting the GPU minify it
        // keeps text edges clean. Only paid while the panel is visible.
        layer.enabled: root.visible
        layer.smooth: true
        layer.mipmap: true

        // Preloaded at startup and kept resident: sync-instantiating this
        // tree on open froze the GUI thread mid-entrance (visible jank).
        // Loaded synchronously at startup: async incubation raced service
        // threads ("Cannot create children for a parent in a different
        // thread") and silently aborted, leaving the panel empty.

        active: true


        sourceComponent: Content {
            screenState: root.screenState
            facePicker: root.facePicker
        }
    }
}

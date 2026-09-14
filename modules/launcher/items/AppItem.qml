import QtQuick
import QtQuick.Layouts
import Quickshell
import Caelestia.Config
import qs.components
import qs.components.images
import qs.services
import qs.utils
import qs.modules.launcher
import qs.modules.launcher.services

LauncherItem {
    id: root

    required property DesktopEntry modelData

    readonly property bool favourite: modelData ? Strings.testRegexList(GlobalConfig.launcher.favouriteApps, modelData.id) : false

    trailing: qsTr("Application")

    onTriggered: {
        Apps.launch(modelData);
        list.screenState.launcher = false;
    }

    Tile {
        CachingIconImage {
            anchors.centerIn: parent
            implicitSize: Style.iconSize
            source: Quickshell.iconPath(root.modelData?.icon, "image-missing")
        }
    }

    StyledText {
        Layout.maximumWidth: Style.panelWidth * 0.4
        text: root.modelData?.name ?? ""
        font: Tokens.font.body.builders.medium.weight(Font.Medium).build()
        elide: Text.ElideRight
    }

    MaterialIcon {
        visible: root.favourite
        text: "favorite"
        fill: 1
        color: Colours.palette.m3primary
        fontStyle: Tokens.font.icon.small
    }

    // Secondary line folded onto the primary one, Raycast style: it takes the
    // slack so the kind label stays pinned to the right edge
    StyledText {
        Layout.fillWidth: true
        text: (root.modelData?.comment || root.modelData?.genericName) ?? ""
        color: Colours.palette.m3onSurfaceVariant
        font: Tokens.font.body.medium
        elide: Text.ElideRight
    }
}

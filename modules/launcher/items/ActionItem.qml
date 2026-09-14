import QtQuick
import QtQuick.Layouts
import Caelestia.Config
import qs.components
import qs.services
import qs.modules.launcher

LauncherItem {
    id: root

    required property var modelData

    trailing: qsTr("Command")

    onTriggered: modelData?.onClicked(list)

    Tile {
        icon: root.modelData?.icon ?? ""
        iconColor: Colours.palette.m3primary
    }

    StyledText {
        Layout.maximumWidth: Style.panelWidth * 0.4
        text: root.modelData?.name ?? ""
        font: Tokens.font.body.builders.medium.weight(Font.Medium).build()
        elide: Text.ElideRight
    }

    StyledText {
        Layout.fillWidth: true
        text: root.modelData?.desc ?? ""
        color: Colours.palette.m3onSurfaceVariant
        font: Tokens.font.body.medium
        elide: Text.ElideRight
    }
}

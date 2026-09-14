import QtQuick
import QtQuick.Layouts
import Caelestia.Config
import qs.components
import qs.services
import qs.modules.launcher
import qs.modules.launcher.services

LauncherItem {
    id: root

    required property M3Variants.Variant modelData

    trailing: qsTr("Variant")

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

    MaterialIcon {
        visible: root.modelData?.variant === Schemes.currentVariant
        text: "check"
        color: Colours.palette.m3primary
        fontStyle: Tokens.font.icon.small
    }

    StyledText {
        Layout.fillWidth: true
        text: root.modelData?.description ?? ""
        font: Tokens.font.body.medium
        color: Colours.palette.m3onSurfaceVariant
        elide: Text.ElideRight
    }
}

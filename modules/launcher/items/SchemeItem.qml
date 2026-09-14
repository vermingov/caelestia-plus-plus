import QtQuick
import QtQuick.Layouts
import Caelestia.Config
import qs.components
import qs.services
import qs.modules.launcher
import qs.modules.launcher.services

LauncherItem {
    id: root

    required property Schemes.Scheme modelData

    trailing: qsTr("Scheme")

    onTriggered: modelData?.onClicked(list)

    StyledRect {
        id: preview

        implicitWidth: Style.iconSize
        implicitHeight: Style.iconSize

        border.width: 1
        border.color: Qt.alpha(`#${root.modelData?.colours?.outline}`, 0.5)

        color: `#${root.modelData?.colours?.surface}`
        radius: Tokens.rounding.full

        // Right half filled with the scheme's primary, so one swatch shows
        // both the surface and the accent it pairs with
        Item {
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.right: parent.right

            implicitWidth: parent.implicitWidth / 2
            clip: true

            StyledRect {
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                anchors.right: parent.right

                implicitWidth: preview.implicitWidth
                color: `#${root.modelData?.colours?.primary}`
                radius: Tokens.rounding.full
            }
        }
    }

    StyledText {
        Layout.maximumWidth: Style.panelWidth * 0.4
        text: root.modelData?.flavour ?? ""
        font: Tokens.font.body.builders.medium.weight(Font.Medium).build()
        elide: Text.ElideRight
    }

    MaterialIcon {
        visible: `${root.modelData?.name} ${root.modelData?.flavour}` === Schemes.currentScheme
        text: "check"
        color: Colours.palette.m3primary
        fontStyle: Tokens.font.icon.small
    }

    StyledText {
        Layout.fillWidth: true
        text: root.modelData?.name ?? ""
        font: Tokens.font.body.medium
        color: Colours.palette.m3onSurfaceVariant
        elide: Text.ElideRight
    }
}

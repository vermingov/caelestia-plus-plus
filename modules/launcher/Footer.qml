import QtQuick
import Caelestia.Config
import qs.components
import qs.services
import qs.modules.launcher

// Action bar along the bottom edge: what Enter will do, and the count of
// what is on offer. Raycast puts its primary action here with its shortcut
// spelled out, which is also the only place a launcher can teach its keys.
Item {
    id: root

    required property string action
    required property int count

    implicitHeight: Style.footerHeight

    StyledText {
        anchors.left: parent.left
        anchors.leftMargin: Style.contentPadding
        anchors.verticalCenter: parent.verticalCenter

        text: root.count === 1 ? qsTr("1 result") : qsTr("%1 results").arg(root.count)
        color: Colours.palette.m3outline
        font: Tokens.font.label.small
    }

    Row {
        anchors.right: parent.right
        anchors.rightMargin: Style.contentPadding
        anchors.verticalCenter: parent.verticalCenter

        spacing: Tokens.spacing.small

        StyledText {
            anchors.verticalCenter: parent.verticalCenter

            animate: true
            text: root.action
            color: Colours.palette.m3onSurfaceVariant
            font: Tokens.font.label.small
        }

        Key {
            anchors.verticalCenter: parent.verticalCenter
            label: "↵"
        }
    }

    component Key: StyledRect {
        property alias label: keyLabel.text

        implicitWidth: Math.max(implicitHeight, keyLabel.implicitWidth + Tokens.padding.small * 2)
        implicitHeight: keyLabel.implicitHeight + Tokens.padding.extraSmall * 2

        radius: Tokens.rounding.small
        color: Qt.alpha(Colours.palette.m3onSurface, 0.08)

        StyledText {
            id: keyLabel

            anchors.centerIn: parent
            color: Colours.palette.m3onSurfaceVariant
            font: Tokens.font.label.small
        }
    }
}

import QtQuick
import Caelestia.Config
import qs.components
import qs.services
import qs.modules.launcher

// The icon slot at the head of a result row. Set `icon` for a Material glyph,
// or place custom content (e.g. a CachingIconImage) as children. No well or
// border behind it: at 22px the icon reads better on the row itself, and a
// framed tile per row is what made the old list look like a settings page.
Item {
    property alias icon: glyph.text
    property alias iconColor: glyph.color

    implicitWidth: Style.iconSize
    implicitHeight: Style.iconSize

    MaterialIcon {
        id: glyph

        anchors.centerIn: parent
        color: Colours.palette.m3onSurfaceVariant
        fontStyle: Tokens.font.icon.small
    }
}

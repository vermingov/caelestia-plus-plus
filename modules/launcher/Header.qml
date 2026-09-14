import QtQuick
import Caelestia.Config
import qs.components
import qs.services
import qs.modules.launcher

// Section label above the results, the way Raycast titles its groups. The
// mode table is shared: Content binds the search field's icon to it and the
// footer names the primary action from it.
Item {
    id: root

    required property string mode

    readonly property var info: {
        const modes = {
            apps: {
                icon: "search",
                label: qsTr("Applications"),
                action: qsTr("Open")
            },
            actions: {
                icon: "bolt",
                label: qsTr("Commands"),
                action: qsTr("Run")
            },
            calc: {
                icon: "function",
                label: qsTr("Calculator"),
                action: qsTr("Copy")
            },
            scheme: {
                icon: "palette",
                label: qsTr("Colour schemes"),
                action: qsTr("Apply")
            },
            variant: {
                icon: "format_paint",
                label: qsTr("Variants"),
                action: qsTr("Apply")
            },
            wallpapers: {
                icon: "wallpaper",
                label: qsTr("Wallpapers"),
                action: qsTr("Set")
            }
        };
        return modes[mode] ?? modes.apps;
    }

    implicitHeight: Style.sectionHeight

    StyledText {
        anchors.left: parent.left
        anchors.leftMargin: Style.contentPadding
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Tokens.padding.extraSmall

        animate: true
        text: root.info.label
        color: Colours.palette.m3outline
        font: Tokens.font.label.small
    }
}

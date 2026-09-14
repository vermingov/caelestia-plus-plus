import QtQuick
import QtQuick.Layouts
import Quickshell
import Caelestia
import Caelestia.Config
import qs.components
import qs.services
import qs.modules.launcher

LauncherItem {
    id: root

    readonly property string math: list.search.text.slice(`${GlobalConfig.launcher.actionPrefix}calc `.length)

    onMathChanged: {
        if (math.length > 0)
            Qalculator.evalAsync(math);
    }

    onTriggered: {
        Quickshell.execDetached(["wl-copy", Qalculator.rawResult]);
        list.screenState.launcher = false;
    }

    Tile {
        icon: "function"
        iconColor: Colours.palette.m3primary
    }

    StyledText {
        id: result

        Layout.fillWidth: true

        color: {
            if (text.includes("error: ") || text.includes("warning: "))
                return Colours.palette.m3error;
            if (!root.math)
                return Colours.palette.m3onSurfaceVariant;
            return Colours.palette.m3onSurface;
        }

        text: root.math.length > 0 ? (Qalculator.result || qsTr("Calculating...")) : qsTr("Type an expression to calculate")
        font: Tokens.font.body.builders.medium.weight(Font.Medium).build()
        elide: Text.ElideLeft
    }

    StyledRect {
        implicitWidth: openLabel.implicitWidth + Tokens.padding.medium * 2
        implicitHeight: openLabel.implicitHeight + Tokens.padding.extraSmall * 2

        visible: root.math.length > 0
        radius: Tokens.rounding.small
        color: openArea.containsMouse ? Qt.alpha(Colours.palette.m3onSurface, 0.1) : "transparent"

        MouseArea {
            id: openArea

            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
                Quickshell.execDetached([...GlobalConfig.general.apps.terminal, "fish", "-C", `exec qalc -i '${root.math}'`]);
                root.list.screenState.launcher = false;
            }
        }

        StyledText {
            id: openLabel

            anchors.centerIn: parent
            text: qsTr("Open in calculator")
            color: Colours.palette.m3onSurfaceVariant
            font: Tokens.font.label.small
        }
    }
}

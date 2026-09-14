import QtQuick
import QtQuick.Layouts
import Caelestia.Config
import qs.components
import qs.services
import qs.modules.launcher

// One result row. Single line, icon then title then a muted inline subtitle,
// with an optional right-aligned kind label — the Raycast shape.
//
// Deliberately no StateLayer: its ripple is a Shape with a radial gradient,
// and a launcher builds a dozen rows per keystroke. Hover is a flat fill and
// the selection highlight does the rest of the work.
Item {
    id: root

    required property int index
    required property var list // AppList root: provides revealing + screenState

    property string trailing

    readonly property alias hovered: hover.hovered
    readonly property alias pressed: tap.pressed
    default property alias content: contentRow.data

    signal triggered()

    // Replays per open via the list's revealing gate. Rows created by search
    // refiltering skip it entirely: replaying a paused stagger on every
    // keystroke churned dozens of animations and produced negative-duration
    // PauseAnimation warnings (see Bible).
    function playEntrance(): void {
        opacity = 0;
        enterSlide.y = Tokens.padding.medium;
        enterAnim.restart();
    }

    anchors.left: parent?.left
    anchors.right: parent?.right
    implicitHeight: Style.rowHeight

    transform: Translate {
        id: enterSlide
    }

    Component.onCompleted: {
        if (list.revealing)
            playEntrance();
    }

    Connections {
        target: root.list

        function onRevealingChanged(): void {
            if (root.list.revealing)
                root.playEntrance();
        }
    }

    SequentialAnimation {
        id: enterAnim

        PauseAnimation {
            duration: Math.max(0, Math.min(root.index, 7)) * 18
        }
        ParallelAnimation {
            Anim {
                target: root
                property: "opacity"
                to: 1
                type: Anim.FastEffects
            }
            Anim {
                target: enterSlide
                property: "y"
                to: 0
                type: Anim.FastSpatial
            }
        }
    }

    // Hover tint only; the current row already carries the selection fill, so
    // painting both would double up on whichever row the pointer rests on
    StyledRect {
        anchors.fill: parent
        anchors.leftMargin: Style.rowInset
        anchors.rightMargin: Style.rowInset

        radius: Style.rowRadius
        color: hover.hovered && root.list.currentIndex !== root.index ? Qt.alpha(Colours.palette.m3onSurface, 0.05) : "transparent"
    }

    HoverHandler {
        id: hover

        onHoveredChanged: {
            if (hovered)
                root.list.currentIndex = root.index;
        }
    }

    TapHandler {
        id: tap

        onTapped: root.triggered()
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Style.contentPadding
        anchors.rightMargin: Style.contentPadding

        spacing: Tokens.spacing.small

        RowLayout {
            id: contentRow

            Layout.fillWidth: true
            spacing: Tokens.spacing.medium
        }

        StyledText {
            visible: root.trailing
            text: root.trailing
            color: Colours.palette.m3outline
            font: Tokens.font.label.small
        }
    }
}

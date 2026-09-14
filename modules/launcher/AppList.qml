pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Caelestia.Config
import qs.components
import qs.components.containers
import qs.components.controls
import qs.services
import qs.modules.launcher
import qs.modules.launcher.items
import qs.modules.launcher.services

StyledListView {
    id: root

    required property SearchBar search
    required property ScreenState screenState

    // True while the launcher is opening: rows replay their staggered
    // entrance. Cleared shortly after so refiltering while typing creates
    // rows without animation churn.
    property bool revealing

    // The list renders `displayText`, which trails `search.text`: it only
    // follows while the launcher is open and the query stays within one
    // state, so a prefix switch re-queries mid-transition (below) instead of
    // flashing the new results under the old delegate, and closing the
    // launcher (which clears the search) keeps the last list for the exit
    property string displayText

    readonly property string requestedState: stateForText(search.text)
    readonly property string displayState: stateForText(displayText)

    function syncDisplayText(): void {
        if (screenState.launcher && requestedState === displayState)
            displayText = search.text;
    }

    function stateForText(text: string): string {
        const prefix = GlobalConfig.launcher.actionPrefix;
        if (text.startsWith(prefix)) {
            for (const action of ["calc", "scheme", "variant"])
                if (text.startsWith(`${prefix}${action} `))
                    return action;

            return "actions";
        }

        return "apps";
    }

    function resultsForText(text: string): var {
        switch (stateForText(text)) {
        case "actions":
            return Actions.query(text);
        case "calc":
            return [0];
        case "scheme":
            return Schemes.query(text);
        case "variant":
            return M3Variants.query(text);
        default:
            return Apps.search(text);
        }
    }

    Connections {
        target: root.search

        function onTextChanged(): void {
            root.syncDisplayText();
        }
    }

    Connections {
        target: root.screenState

        function onLauncherChanged(): void {
            root.syncDisplayText();
            if (root.screenState.launcher) {
                root.revealing = true;
                revealTimer.restart();
            } else {
                root.revealing = false;
                revealTimer.stop();
            }
        }
    }

    Timer {
        id: revealTimer

        interval: 400
        onTriggered: root.revealing = false
    }

    model: ScriptModel {
        values: root.resultsForText(root.displayText)
        onValuesChanged: root.currentIndex = 0
    }

    spacing: 0
    orientation: Qt.Vertical
    implicitHeight: Style.rowHeight * Math.min(Config.launcher.maxShown, count)

    preferredHighlightBegin: 0
    preferredHighlightEnd: height
    highlightRangeMode: ListView.ApplyRange

    highlightFollowsCurrentItem: false
    highlight: Item {
        y: root.currentItem?.y ?? 0
        implicitWidth: root.width
        implicitHeight: root.currentItem?.implicitHeight ?? 0

        // Only the travel is animated; fading the fill in and out as well
        // made every keystroke flash the whole row
        Behavior on y {
            Anim {
                type: Anim.EmphasizedSmall
            }
        }

        // Glass on glass: a smaller pane sitting on the panel's, lit the same
        // way, rather than a flat fill that reads as a highlighter mark.
        GlassSurface {
            anchors.fill: parent
            anchors.leftMargin: Style.rowInset
            anchors.rightMargin: Style.rowInset

            radius: Style.rowRadius
            lift: 0.05
            rim: 0.45
            lens: 0.4
            scrim: 0
            band: 9
            grain: 0
        }
    }

    state: screenState.launcher ? requestedState : displayState

    onStateChanged: {
        if (state === "scheme" || state === "variant")
            Schemes.reload();
    }

    Component.onCompleted: displayText = search.text

    states: [
        State {
            name: "apps"

            PropertyChanges {
                root.delegate: appItem
            }
        },
        State {
            name: "actions"

            PropertyChanges {
                root.delegate: actionItem
            }
        },
        State {
            name: "calc"

            PropertyChanges {
                root.delegate: calcItem
            }
        },
        State {
            name: "scheme"

            PropertyChanges {
                root.delegate: schemeItem
            }
        },
        State {
            name: "variant"

            PropertyChanges {
                root.delegate: variantItem
            }
        }
    ]

    transitions: Transition {
        SequentialAnimation {
            ParallelAnimation {
                Anim {
                    target: root
                    property: "opacity"
                    from: 1
                    to: 0
                    duration: Tokens.anim.durations.small
                    easing: Tokens.anim.standardAccel
                }
                Anim {
                    target: root
                    property: "scale"
                    from: 1
                    to: 0.98
                    duration: Tokens.anim.durations.small
                    easing: Tokens.anim.standardAccel
                }
            }
            PropertyAction {
                target: root
                property: "delegate"
                value: null
            }
            ScriptAction {
                script: root.displayText = root.search.text
            }
            PropertyAction {
                target: root
                property: "delegate"
            }
            ParallelAnimation {
                Anim {
                    target: root
                    property: "opacity"
                    from: 0
                    to: 1
                    duration: Tokens.anim.durations.small
                    easing: Tokens.anim.standardDecel
                }
                Anim {
                    target: root
                    property: "scale"
                    from: 0.98
                    to: 1
                    duration: Tokens.anim.durations.small
                    easing: Tokens.anim.standardDecel
                }
            }
            PropertyAction {
                targets: [root.add, root.remove]
                property: "enabled"
                value: true
            }
        }
    }

    StyledScrollBar.vertical: StyledScrollBar {
        flickable: root
    }

    add: Transition {
        enabled: !root.state

        Anim {
            type: Anim.DefaultEffects
            property: "opacity"
            from: 0
            to: 1
        }
    }

    remove: Transition {
        enabled: !root.state

        Anim {
            type: Anim.DefaultEffects
            property: "opacity"
            from: 1
            to: 0
        }
    }

    move: Transition {
        Anim {
            property: "y"
        }
        Anim {
            type: Anim.DefaultEffects
            property: "opacity"
            to: 1
        }
    }

    addDisplaced: Transition {
        Anim {
            property: "y"
            type: Anim.StandardSmall
        }
        Anim {
            type: Anim.DefaultEffects
            property: "opacity"
            to: 1
        }
    }

    displaced: Transition {
        Anim {
            property: "y"
        }
        Anim {
            type: Anim.DefaultEffects
            property: "opacity"
            to: 1
        }
    }

    Component {
        id: appItem

        AppItem {
            list: root
        }
    }

    Component {
        id: actionItem

        ActionItem {
            list: root
        }
    }

    Component {
        id: calcItem

        CalcItem {
            list: root
        }
    }

    Component {
        id: schemeItem

        SchemeItem {
            list: root
        }
    }

    Component {
        id: variantItem

        VariantItem {
            list: root
        }
    }
}

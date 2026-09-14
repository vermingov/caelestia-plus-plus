pragma ComponentBehavior: Bound

import QtQuick
import Caelestia.Config
import qs.components
import qs.components.controls
import qs.components.effects
import qs.services
import qs.modules.launcher
import qs.modules.launcher.services

// One surface: search, results and action bar inside a single frosted panel.
// The old layout floated the search field below the results as a separate
// pill, which is what made it read as two unrelated widgets.
//
// The panel fill must stay above the compositor's ignore_alpha threshold
// (Colours.reloadHyprRules sets it to transparency.base - 0.03) or Hyprland
// stops blurring behind it and the frost disappears.
Item {
    id: root

    required property ScreenState screenState
    required property var panels
    required property real maxHeight
    property real openProgress: 1

    readonly property int rounding: Tokens.rounding.extraLarge

    implicitWidth: Style.panelWidth
    implicitHeight: Style.searchHeight + listArea.implicitHeight + Style.footerHeight + 2

    Elevation {
        anchors.fill: panel
        radius: panel.radius
        level: 3
        opacity: root.openProgress
    }

    StyledClippingRect {
        id: panel

        anchors.fill: parent

        radius: root.rounding
        color: Colours.tPalette.m3surfaceContainer

        Item {
            id: search

            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right

            implicitHeight: Style.searchHeight

            SearchBar {
                id: searchField

                objectName: "launcherSearch"

                anchors.fill: parent

                topPadding: 0
                bottomPadding: 0
                font: Tokens.font.body.large
                placeholderText: qsTr("Search for apps and commands…")

                // Flat inside the panel: the panel is the surface, the field
                // is just text on it
                bg.color: "transparent"
                bg.radius: 0

                onAccepted: {
                    const currentItem = list.currentList?.currentItem;
                    if (!currentItem)
                        return;

                    if (list.showWallpapers) {
                        if (Colours.scheme === "dynamic" && currentItem.modelData.path !== Wallpapers.actualCurrent)
                            Wallpapers.previewColourLock = true;
                        Wallpapers.setWallpaper(currentItem.modelData.path);
                        root.screenState.launcher = false;
                    } else if (text.startsWith(GlobalConfig.launcher.actionPrefix)) {
                        if (text.startsWith(`${GlobalConfig.launcher.actionPrefix}calc `))
                            currentItem.onClicked();
                        else
                            currentItem.modelData.onClicked(list.currentList);
                    } else {
                        Apps.launch(currentItem.modelData);
                        root.screenState.launcher = false;
                    }
                }

                Keys.onUpPressed: list.currentList?.decrementCurrentIndex()
                Keys.onDownPressed: list.currentList?.incrementCurrentIndex()

                Keys.onEscapePressed: root.screenState.launcher = false

                Keys.onPressed: event => {
                    if (!GlobalConfig.launcher.vimKeybinds)
                        return;

                    if (event.modifiers & Qt.ControlModifier) {
                        if (event.key === Qt.Key_J || event.key === Qt.Key_N) {
                            list.currentList?.incrementCurrentIndex();
                            event.accepted = true;
                        } else if (event.key === Qt.Key_K || event.key === Qt.Key_P) {
                            list.currentList?.decrementCurrentIndex();
                            event.accepted = true;
                        }
                    } else if (event.key === Qt.Key_Tab) {
                        list.currentList?.incrementCurrentIndex();
                        event.accepted = true;
                    } else if (event.key === Qt.Key_Backtab || (event.key === Qt.Key_Tab && (event.modifiers & Qt.ShiftModifier))) {
                        list.currentList?.decrementCurrentIndex();
                        event.accepted = true;
                    }
                }

                // Content is preloaded and kept resident (see Wrapper), so the
                // search field must retake focus on every open, not just on creation
                Component.onCompleted: {
                    if (root.screenState.launcher)
                        forceActiveFocus();
                }

                Connections {
                    function onLauncherChanged(): void {
                        if (root.screenState.launcher)
                            searchField.forceActiveFocus();
                        else
                            searchField.text = "";
                    }

                    function onSessionChanged(): void {
                        if (!root.screenState.session && root.screenState.launcher)
                            searchField.forceActiveFocus();
                    }

                    target: root.screenState
                }
            }
        }

        Separator {
            id: topRule

            anchors.top: search.bottom
        }

        Item {
            id: listArea

            anchors.top: topRule.bottom
            anchors.left: parent.left
            anchors.right: parent.right

            implicitHeight: header.implicitHeight + list.height

            Header {
                id: header

                anchors.top: parent.top
                anchors.left: parent.left
                anchors.right: parent.right

                mode: list.mode
            }

            ContentList {
                id: list

                anchors.top: header.bottom

                content: root
                screenState: root.screenState
                panels: root.panels
                maxHeight: root.maxHeight - Style.searchHeight - Style.footerHeight - header.implicitHeight - 2
                search: searchField
            }
        }

        Separator {
            id: bottomRule

            anchors.top: listArea.bottom
        }

        Footer {
            id: footer

            anchors.top: bottomRule.bottom
            anchors.left: parent.left
            anchors.right: parent.right

            action: header.info.action
            count: list.resultCount
        }
    }

    // Hairline over the frosted fill, the edge that gives the panel its shape
    StyledRect {
        anchors.fill: panel

        radius: panel.radius
        color: "transparent"
        border.width: 1
        border.color: Qt.alpha(Colours.palette.m3onSurface, 0.1)
    }

    // Search icon follows the active mode: mode glyph in primary while a
    // command prefix is active, plain search otherwise.
    Binding {
        target: searchField.searchIcon
        property: "animate"
        value: true
    }

    Binding {
        target: searchField.searchIcon
        property: "text"
        value: list.mode === "apps" ? "search" : header.info.icon
    }

    Binding {
        target: searchField.searchIcon
        property: "color"
        value: list.mode === "apps" ? Colours.palette.m3onSurfaceVariant : Colours.palette.m3primary
    }

    // SearchBar asks for Tokens.font.icon.builders.medium, which the plugin
    // does not export — the resulting font is undefined
    Binding {
        target: searchField.searchIcon
        property: "fontStyle"
        value: Tokens.font.icon.small
    }

    component Separator: StyledRect {
        anchors.left: parent.left
        anchors.right: parent.right

        implicitHeight: 1
        color: Qt.alpha(Colours.palette.m3outlineVariant, 0.22)
    }
}

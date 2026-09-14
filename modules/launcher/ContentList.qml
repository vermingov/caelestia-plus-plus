pragma ComponentBehavior: Bound

import QtQuick
import Caelestia.Config
import qs.components
import qs.components.controls
import qs.services
import qs.utils
import qs.modules.launcher

// Swaps the result list between the app/action list and the wallpaper
// browser. The panel width is fixed, so only the height moves — a launcher
// that changes width while you type is the jumpiest thing a launcher can do.
Item {
    id: root

    required property var content
    required property ScreenState screenState
    required property var panels
    required property real maxHeight
    required property SearchBar search

    readonly property bool showWallpapers: search.text.startsWith(`${GlobalConfig.launcher.actionPrefix}wallpaper `)
    readonly property var currentList: showWallpapers ? wallpaperList.item : appList.item // Can be either ListView or PathView, so can't type properly
    readonly property string mode: showWallpapers ? "wallpapers" : (appList.item?.state ?? "apps")
    readonly property int resultCount: currentList?.count ?? 0
    property string animState: showWallpapers ? "wallpapers" : "apps"

    anchors.left: parent.left
    anchors.right: parent.right

    clip: true
    state: animState

    states: [
        State {
            name: "apps"

            PropertyChanges {
                root.implicitHeight: Math.min(root.maxHeight, appList.implicitHeight > 0 ? appList.implicitHeight : empty.implicitHeight)
                appList.active: true
            }
        },
        State {
            name: "wallpapers"

            PropertyChanges {
                root.implicitHeight: Math.min(root.maxHeight, Tokens.sizes.launcher.wallpaperHeight)
                wallpaperList.active: true
            }
        }
    ]

    Behavior on animState {
        SequentialAnimation {
            Anim {
                target: root
                property: "opacity"
                from: 1
                to: 0
                type: Anim.FastEffects
            }
            PropertyAction {}
            Anim {
                target: root
                property: "opacity"
                from: 0
                to: 1
                type: Anim.FastEffects
            }
        }
    }

    Loader {
        id: appList

        active: false

        anchors.fill: parent

        sourceComponent: AppList {
            objectName: "launcherAppList"

            search: root.search
            screenState: root.screenState
        }
    }

    Loader {
        id: wallpaperList

        asynchronous: true
        active: false

        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.horizontalCenter: parent.horizontalCenter

        sourceComponent: WallpaperList {
            objectName: "launcherWallpaperList"

            search: root.search
            screenState: root.screenState
            panels: root.panels
            content: root.content
        }
    }

    Column {
        id: empty

        anchors.centerIn: parent
        spacing: Tokens.spacing.extraSmall
        padding: Tokens.padding.extraLarge

        opacity: root.currentList?.count === 0 ? 1 : 0

        StyledText {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.state === "wallpapers" ? qsTr("No wallpapers found") : qsTr("No results")
            color: Colours.palette.m3onSurfaceVariant
            font: Tokens.font.body.builders.medium.weight(Font.Medium).build()
        }

        StyledText {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.state === "wallpapers" && Wallpapers.list.length === 0 ? qsTr("Try putting some wallpapers in %1").arg(Paths.shortenHome(Paths.wallsdir)) : qsTr("Try a different search")
            color: Colours.palette.m3outline
            font: Tokens.font.body.medium
        }

        Behavior on opacity {
            Anim {
                type: Anim.FastEffects
            }
        }
    }

    Behavior on implicitHeight {
        enabled: root.screenState.launcher

        Anim {
            type: Anim.EmphasizedSmall
        }
    }
}

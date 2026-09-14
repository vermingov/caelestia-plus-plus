pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import Caelestia.Config
import qs.components
import qs.services

Item {
    id: root

    required property ShellScreen screen
    required property real offsetScale

    readonly property alias content: content
    readonly property alias nexus: nexus

    readonly property real nonAnimWidth: children.find(c => c.shouldBeActive)?.implicitWidth ?? content.implicitWidth
    readonly property real nonAnimHeight: children.find(c => c.shouldBeActive)?.implicitHeight ?? content.implicitHeight
    readonly property Item current: (content.item as Content)?.current ?? null
    readonly property bool isDetached: detachedMode.length > 0

    property alias currentName: popoutState.currentName
    property alias hasCurrent: popoutState.hasCurrent
    property real currentCenter
    // Last x Bar.checkPopout received — diagnostic breadcrumb for the
    // `popouts` IPC below; -1 means hover events never reach the bar
    property real lastCheckX: -1

    property string detachedMode
    property string queuedMode

    // Dummy object so Tokens attached prop resolves to global config
    // Anim configs are not per-monitor
    readonly property QtObject dummy: QtObject {}
    property int animLength: dummy.Tokens.anim.durations.expressiveDefaultSpatial
    property var animCurve: dummy.Tokens.anim.expressiveDefaultSpatial // The easingCurve type is Qt 6.11+ so we gotta use var for now

    function setAnims(detach: bool): void {
        const type = `expressive${detach ? "Slow" : "Default"}Spatial`;
        animLength = dummy.Tokens.anim.durations[type];
        animCurve = dummy.Tokens.anim[type];
    }

    function detach(mode: string): void {
        setAnims(true);
        queuedMode = mode;
        detachedMode = "any";
        setAnims(false);
        focus = true;
    }

    function close(): void {
        hasCurrent = false;
        detachedMode = "";
    }

    // Remote diagnosis for dead hover popouts — one target per screen, since
    // every monitor has its own wrapper and the failure can be per-screen:
    //   qs -c caelestia ipc call popouts-<monitor> open network
    //   qs -c caelestia ipc call popouts-<monitor> state
    // (monitor names from: hyprctl monitors -j | jq -r '.[].name')
    IpcHandler {
        target: `popouts-${root.screen.name}`

        function open(name: string): void {
            root.currentCenter = root.screen.width / 2;
            root.currentName = name;
            root.hasCurrent = true;
        }

        function close(): void {
            root.close();
        }

        function state(): string {
            return JSON.stringify({
                screen: root.screen.name,
                hasCurrent: root.hasCurrent,
                currentName: root.currentName,
                lastCheckX: root.lastCheckX,
                statusIconsPopouts: root.dummy.Config.bar.popouts.statusIcons
            });
        }
    }

    implicitWidth: nonAnimWidth
    implicitHeight: nonAnimHeight

    focus: hasCurrent
    Keys.onEscapePressed: {
        // Forward escape to password popout if active, otherwise close
        if (currentName === "wirelesspassword" && content.item) {
            const passwordPopout = (content.item as Content)?.children.find(c => c.name === "wirelesspassword");
            if (passwordPopout && passwordPopout.item) {
                passwordPopout.item.closeDialog();
                return;
            }
        }
        close();
    }

    Keys.onPressed: event => {
        // Don't intercept keys when password popout is active - let it handle them
        if (currentName === "wirelesspassword") {
            event.accepted = false;
        }
    }

    PopoutState {
        id: popoutState

        onDetachRequested: mode => root.detach(mode)
    }

    HyprlandFocusGrab {
        active: root.isDetached
        windows: [QsWindow.window]
        onCleared: root.close()
    }

    Binding {
        when: root.isDetached || (root.hasCurrent && root.currentName === "wirelesspassword")

        target: QsWindow.window
        property: "WlrLayershell.keyboardFocus"
        value: WlrKeyboardFocus.OnDemand
    }

    Comp {
        id: content

        shouldBeActive: root.hasCurrent && !root.detachedMode
        anchors.fill: parent

        sourceComponent: Content {
            popouts: popoutState
        }
    }

    Comp {
        id: nexus

        readonly property int queuedPageIdx: ["appearance", "network", "bluetooth", "audio"].indexOf(root.queuedMode)

        shouldBeActive: root.detachedMode === "any"
        anchors.centerIn: parent

        // Loaded by URL so the settings module stays out of the startup
        // compile (shell.qml precompiles it off the GUI thread after the
        // first frame). setSource hands over the initial values; the source
        // is cleared on close so the next open goes through here again.
        onActiveChanged: {
            if (active)
                setSource("NexusHost.qml", {
                    screen: root.screen,
                    animating: nexus.opacity < 1,
                    pageIdx: nexus.queuedPageIdx
                });
            else
                source = "";
        }

        Binding {
            target: nexus.item
            property: "animating"
            value: nexus.opacity < 1
        }

        Binding {
            target: nexus.item
            property: "pageIdx"
            value: nexus.queuedPageIdx
        }

        Connections {
            target: nexus.item

            function onClose(): void {
                root.close();
            }
        }
    }

    Behavior on implicitWidth {
        Anim {
            duration: root.animLength
            easing: root.animCurve
        }
    }

    Behavior on implicitHeight {
        enabled: root.offsetScale < 1

        Anim {
            duration: root.animLength
            easing: root.animCurve
        }
    }

    component Comp: Loader {
        id: comp

        property bool shouldBeActive

        active: false
        opacity: 0

        // Makes the loader load on the same frame shouldBeActive becomes true, which ensures size is set
        states: State {
            name: "active"
            when: comp.shouldBeActive

            PropertyChanges {
                comp.opacity: 1
                comp.active: true
            }
        }

        transitions: [
            Transition {
                from: ""
                to: "active"

                SequentialAnimation {
                    PropertyAction {
                        property: "active"
                    }
                    Anim {
                        type: Anim.DefaultEffects
                        property: "opacity"
                    }
                }
            },
            Transition {
                from: "active"
                to: ""

                SequentialAnimation {
                    Anim {
                        type: Anim.DefaultEffects
                        property: "opacity"
                    }
                    PropertyAction {
                        property: "active"
                    }
                }
            }
        ]
    }
}

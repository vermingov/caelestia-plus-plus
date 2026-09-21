pragma ComponentBehavior: Bound

import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.components.containers
import qs.components.misc
import qs.services

Scope {
    id: picker

    LazyLoader {
        id: root

        property bool freeze
        property bool closing
        property bool clipboardOnly

        Variants {
            model: Screens.screens

            StyledWindow {
                id: win

                required property ShellScreen modelData

                screen: modelData
                name: "area-picker"
                WlrLayershell.exclusionMode: ExclusionMode.Ignore
                WlrLayershell.layer: WlrLayer.Overlay
                WlrLayershell.keyboardFocus: root.closing ? WlrKeyboardFocus.None : WlrKeyboardFocus.Exclusive
                mask: root.closing ? empty : null

                anchors.top: true
                anchors.bottom: true
                anchors.left: true
                anchors.right: true

                Region {
                    id: empty
                }

                Picker {
                    loader: root
                    screen: win.modelData
                }
            }
        }
    }

    // Opens it, here or in the external bar where that is the one with a
    // picker: two pickers over the same screen would each take a shot of the
    // other.
    function open(freeze: bool, clip: bool): void {
        if (ExternalBar.hasPicker) {
            ExternalBar.ask(["picker", ...(freeze ? ["freeze"] : []), ...(clip ? ["clip"] : [])]);
            return;
        }
        root.freeze = freeze;
        root.closing = false;
        root.clipboardOnly = clip;
        root.activeAsync = true;
    }

    IpcHandler {
        function open(): void {
            picker.open(false, false);
        }

        function openFreeze(): void {
            picker.open(true, false);
        }

        function openClip(): void {
            picker.open(false, true);
        }

        function openFreezeClip(): void {
            picker.open(true, true);
        }

        target: "picker"
    }

    // The four ways in, each of which the external bar's own picker takes
    // over when it has one. `open` below is what does the handing over.
    component Shot: CustomShortcut {
        required property bool freeze
        required property bool clip

        onPressed: picker.open(freeze, clip)
    }

    // qmllint disable unresolved-type
    Shot {
        // qmllint enable unresolved-type
        name: "screenshot"
        description: "Open screenshot tool"
        freeze: false
        clip: false
    }

    // qmllint disable unresolved-type
    Shot {
        // qmllint enable unresolved-type
        name: "screenshotFreeze"
        description: "Open screenshot tool (freeze mode)"
        freeze: true
        clip: false
    }

    // qmllint disable unresolved-type
    Shot {
        // qmllint enable unresolved-type
        name: "screenshotClip"
        description: "Open screenshot tool (clipboard)"
        freeze: false
        clip: true
    }

    // qmllint disable unresolved-type
    Shot {
        // qmllint enable unresolved-type
        name: "screenshotFreezeClip"
        description: "Open screenshot tool (freeze mode, clipboard)"
        freeze: true
        clip: true
    }
}

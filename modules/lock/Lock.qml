pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.components.misc
import qs.services

Scope {
    id: root

    property alias lock: lock

    // Locking is the external bar's while it draws its own lock screen: two
    // session locks is one too many, and the compositor only takes the first.
    function ask(locked: bool): void {
        if (ExternalBar.hasLock) {
            ExternalBar.ask([locked ? "lock" : "unlock"]);
            return;
        }
        if (locked)
            lock.locked = true;
        else
            lock.unlock();
    }

    WlSessionLock {
        id: lock

        signal unlock

        LockSurface {
            lock: lock
            pam: pam
        }
    }

    Pam {
        id: pam

        lock: lock
    }

    Loader {
        asynchronous: true
        active: true
        onLoaded: active = false

        // Force a load of a screencopy so the one in the lock works
        // My guess is the ICC backend loads async on first request, which if the lock is
        // the first request it fails to capture (because it's async and the compositor
        // refuses capture when locked). Warm every screen — capture state is
        // per-output, so warming only the first left other monitors cold.
        sourceComponent: Item {
            Repeater {
                model: Quickshell.screens

                ScreencopyView {
                    required property ShellScreen modelData

                    captureSource: modelData
                }
            }
        }
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "lock"
        description: "Lock the current session"
        onPressed: root.ask(true)
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "unlock"
        description: "Unlock the current session"
        onPressed: root.ask(false)
    }

    IpcHandler {
        function lock(): void {
            root.ask(true);
        }

        function unlock(): void {
            root.ask(false);
        }

        function isLocked(): bool {
            return lock.locked;
        }

        target: "lock"
    }
}

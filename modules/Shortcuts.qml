import QtQuick
import Quickshell
import Quickshell.Io
import Caelestia
import qs.components.misc
import qs.services
import qs.modules.nexus

Scope {
    id: root

    property bool launcherInterrupted
    readonly property bool hasFullscreen: Hypr.focusedWorkspace?.toplevels.values.some(t => t.lastIpcObject.fullscreen > 1) ?? false

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "nexus"
        description: "Open nexus"
        onPressed: WindowFactory.create()
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "showall"
        description: "Toggle launcher, dashboard and osd"
        onPressed: {
            if (root.hasFullscreen)
                return;
            const v = ShellState.forActive();
            const showing = !(v.launcher || v.dashboard || v.osd || v.utilities);
            const how = showing ? "show" : "hide";
            // Whatever the installed shell draws, it is asked for; the rest
            // is this shell's own.
            for (const piece of ["dashboard", "osd", "utilities"])
                if (!ExternalBar.asked(piece, how))
                    v[piece] = showing;
            v.launcher = showing;
        }
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "dashboard"
        description: "Toggle dashboard"
        onPressed: {
            if (root.hasFullscreen)
                return;
            if (ExternalBar.asked("dashboard", "toggle"))
                return;
            const screenState = ShellState.forActive();
            screenState.dashboard = !screenState.dashboard;
        }
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "session"
        description: "Toggle session menu"
        onPressed: {
            if (root.hasFullscreen)
                return;
            if (ExternalBar.asked("session", "toggle"))
                return;
            const screenState = ShellState.forActive();
            screenState.session = !screenState.session;
        }
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "launcher"
        description: "Toggle launcher"
        // The launcher is its own process now (launcher/, a Tauri app on an
        // overlay layer surface). The shortcut still lives here because this
        // is where the tap-versus-hold logic is: SUPER on its own opens it,
        // SUPER as a modifier does not.
        //
        // The QML launcher is still in the tree and still works; it takes
        // over whenever the binary is not installed.
        onPressed: root.launcherInterrupted = false
        onReleased: {
            if (!root.launcherInterrupted && !root.hasFullscreen) {
                // `ready` matters: the check for the binary is a process, and
                // until it has answered `external` is false — which used to
                // send the very first keypress of a session to the QML
                // launcher. Nothing is lost by treating "not yet known" as
                // external, because the socket send falls back to spawning
                // the binary, which fails harmlessly if there is none.
                if (Launcher.external || !Launcher.ready)
                    Launcher.toggle();
                else {
                    const screenState = ShellState.forActive();
                    screenState.launcher = !screenState.launcher;
                }
            }
            root.launcherInterrupted = false;
        }
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "launcherInterrupt"
        description: "Interrupt launcher keybind"
        onPressed: root.launcherInterrupted = true
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "sidebar"
        description: "Toggle sidebar"
        onPressed: {
            if (root.hasFullscreen)
                return;
            // The bar's notification centre, while the bar is the one
            // drawing it. Asked for directly rather than through the flag
            // below, because the flag can only ever open it.
            if (Notifs.external) {
                Notifs.toggleCentre();
                return;
            }
            const screenState = ShellState.forActive();
            screenState.sidebar = !screenState.sidebar;
        }
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "utilities"
        description: "Toggle utilities"
        onPressed: {
            if (root.hasFullscreen || ExternalBar.asked("utilities", "toggle"))
                return;
            const screenState = ShellState.forActive();
            screenState.utilities = !screenState.utilities;
        }
    }

    IpcHandler {
        function toggle(drawer: string): void {
            if (list().split("\n").includes(drawer)) {
                if (root.hasFullscreen && ["launcher", "session", "dashboard"].includes(drawer))
                    return;
                if (ExternalBar.asked(drawer, "toggle"))
                    return;
                const screenState = ShellState.forActive();
                screenState[drawer] = !screenState[drawer];
            } else {
                console.warn(lc, `Drawer "${drawer}" does not exist`);
            }
        }

        function list(): string {
            const screenState = ShellState.forActive();
            return Object.keys(screenState).filter(k => typeof screenState[k] === "boolean").join("\n");
        }

        function isOpen(drawer: string): string {
            const screenState = ShellState.forActive();
            if (typeof screenState[drawer] !== "boolean")
                return "unknown";
            return screenState[drawer] ? "1" : "0";
        }

        target: "drawers"
    }

    IpcHandler {
        function open(): void {
            WindowFactory.create();
        }

        target: "nexus"
    }

    // cae asks this before it draws something that is also drawn here, and
    // draws it only on a yes. A shell from before this handler existed says
    // "Target not found", which is a no: that shell has not stood anything
    // down, and the desktop would have two of whatever it was.
    IpcHandler {
        function stoodDown(what: string): string {
            return ExternalBar.has(what) ? "1" : "0";
        }

        target: "cae"
    }

    IpcHandler {
        function info(title: string, message: string, icon: string): void {
            Toaster.toast(title, message, icon, Toast.Info);
        }

        function success(title: string, message: string, icon: string): void {
            Toaster.toast(title, message, icon, Toast.Success);
        }

        function warn(title: string, message: string, icon: string): void {
            Toaster.toast(title, message, icon, Toast.Warning);
        }

        function error(title: string, message: string, icon: string): void {
            Toaster.toast(title, message, icon, Toast.Error);
        }

        target: "toaster"
    }

    LoggingCategory {
        id: lc

        name: "caelestia.qml.shortcuts"
        defaultLogLevel: LoggingCategory.Info
    }
}

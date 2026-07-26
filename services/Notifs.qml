pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Notifications
import Caelestia
import Caelestia.Config
import qs.components.misc
import qs.services
import qs.utils

Singleton {
    id: root

    property list<NotifData> list: []
    readonly property list<NotifData> notClosed: list.filter(n => !n.closed)
    readonly property list<NotifData> popups: list.filter(n => n.popup)
    property alias dnd: props.dnd

    property bool loaded

    function hasFullscreen(): bool {
        for (const monitor of Hypr.monitors.values) {
            if (monitor?.activeWorkspace?.toplevels.values.some(t => t.lastIpcObject.fullscreen > 1))
                return true;
        }
        return false;
    }

    function shouldShowPopup(): bool {
        if (props.dnd || ShellState.anySidebarOpen())
            return false;
        if (GlobalConfig.notifs.fullscreen === "off" && hasFullscreen())
            return false;
        return true;
    }

    onDndChanged: {
        if (!GlobalConfig.utilities.toasts.dndChanged)
            return;

        if (dnd)
            Toaster.toast(qsTr("Do not disturb enabled"), qsTr("Popup notifications are now disabled"), "do_not_disturb_on");
        else
            Toaster.toast(qsTr("Do not disturb disabled"), qsTr("Popup notifications are now enabled"), "do_not_disturb_off");
    }

    onListChanged: {
        if (loaded)
            saveTimer.restart();
    }

    Timer {
        id: saveTimer

        interval: 1000
        onTriggered: storage.setText(JSON.stringify(root.notClosed.map(n => ({
                    time: n.time,
                    id: n.id,
                    summary: n.summary,
                    body: n.body,
                    appIcon: n.appIcon,
                    appName: n.appName,
                    image: n.image,
                    expireTimeout: n.expireTimeout,
                    urgency: n.urgency,
                    resident: n.resident,
                    hasActionIcons: n.hasActionIcons,
                    actions: n.actions
                }))))
    }

    PersistentProperties {
        id: props

        property bool dnd

        reloadableId: "notifs"
    }

    // This is the user's main shell — it owns notifications, full stop. The
    // server lives in a Loader so it can be rebuilt to (re)bind the D-Bus
    // name: if a foreign daemon (dunst/mako/swaync…) grabbed
    // org.freedesktop.Notifications first, the grabber below stops it and
    // rebuilds this, binding the freed name. quickshell then holds the name,
    // so a respawning daemon can't take it back. No manual "reclaim" needed.
    Loader {
        id: serverLoader

        active: true

        sourceComponent: NotificationServer {
            keepOnReload: false
            actionsSupported: true
            bodyHyperlinksSupported: true
            bodyImagesSupported: true
            bodyMarkupSupported: true
            imageSupported: true
            persistenceSupported: true

            onNotification: notif => {
                notif.tracked = true;

                const comp = notifComp.createObject(root, {
                    popup: root.shouldShowPopup(),
                    notification: notif
                });
                root.list = [comp, ...root.list];
            }
        }
    }

    function _rebindServer(): void {
        serverLoader.active = false;
        Qt.callLater(() => serverLoader.active = true);
    }

    // Stops whoever owns the notification service if it isn't us, so the
    // server can claim it. Exits 10 when it displaced a competitor (rebind
    // needed), 0 when we already own it / it's free / nobody could be found.
    Process {
        id: grabber

        command: ["sh", "-c", `owner=$(busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus GetNameOwner s org.freedesktop.Notifications 2>/dev/null | awk '{print $2}' | tr -d '"')
[ -z "$owner" ] && exit 0
pid=$(busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus GetConnectionUnixProcessID s "$owner" 2>/dev/null | awk '{print $2}')
[ -z "$pid" ] && exit 0
[ "$pid" = "${Quickshell.processId}" ] && exit 0
# Displacing a notification daemon (dunst/mako/swaync/…) is the whole point,
# but some desktops hand this name to the session shell itself. Killing that
# takes the entire desktop down, so a compositor/session process is never a
# target — no matter what the cgroup below says.
comm=$(cat "/proc/$pid/comm" 2>/dev/null)
case "$comm" in
    Hyprland|sway|river|niri|labwc|weston|plasmashell|kwin_wayland|kwin_x11|gnome-shell|xfce4-session|cinnamon-session|mate-session|lxqt-session|systemd|init)
        echo "refusing to displace session process $comm (pid $pid)" >&2
        exit 0 ;;
esac
# A daemon started from the compositor shares its cgroup unit; never stop
# that (it would kill the desktop) — only a dedicated notification unit.
unit=$(grep -oE '[a-zA-Z0-9@._-]+\\.service' "/proc/$pid/cgroup" 2>/dev/null | grep -viE 'user@|wayland-wm|graphical-session|hyprland|plasma|gnome-session|session\\.slice|init\\.scope' | head -1)
[ -n "$unit" ] && systemctl --user stop "$unit" 2>/dev/null
kill "$pid" 2>/dev/null
sleep 1
kill -9 "$pid" 2>/dev/null
exit 10`]

        onExited: code => {
            if (code === 10)
                root._rebindServer();
        }
    }

    // Grab at startup (after the server's own first bind attempt) and re-check
    // periodically — a no-op once we hold the name.
    Timer {
        running: true
        interval: 2000
        onTriggered: grabber.running = true
    }

    Timer {
        running: true
        repeat: true
        interval: 60000
        onTriggered: grabber.running = true
    }

    FileView {
        id: storage

        printErrors: false
        path: `${Paths.state}/notifs.json`
        onLoaded: {
            const data = JSON.parse(text());
            for (const notif of data) {
                const properties = Object.assign({}, notif);

                // Backwards compatibility for old notifications
                if (properties.notificationId === undefined && properties.id !== undefined)
                    properties.notificationId = properties.id;

                delete properties.id;
                root.list.push(notifComp.createObject(root, properties));
            }
            root.list.sort((a, b) => b.time - a.time);
            root.loaded = true;
        }
        onLoadFailed: err => {
            if (err === FileViewError.FileNotFound) {
                root.loaded = true;
                Qt.callLater(() => setText("[]"));
            }
        }
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "clearNotifs"
        description: "Clear all notifications"
        onPressed: {
            for (const notif of root.list.slice())
                notif.close();
        }
    }

    IpcHandler {
        function clear(): void {
            for (const notif of root.list.slice())
                notif.close();
        }

        function isDndEnabled(): bool {
            return props.dnd;
        }

        function toggleDnd(): void {
            props.dnd = !props.dnd;
        }

        function enableDnd(): void {
            props.dnd = true;
        }

        function disableDnd(): void {
            props.dnd = false;
        }

        target: "notifs"
    }

    Component {
        id: notifComp

        NotifData {}
    }
}

import QtQuick
import Quickshell
import Caelestia.Config
import qs.services

// Singletons only instantiate on first reference, so a service nothing
// resident touches (the notification server, idle inhibitor, VPN watcher)
// would otherwise not exist until some panel happened to open
Scope {
    Component.onCompleted: {
        IdleInhibitor;
        GameMode;
        Notifs;
        Players;
        Brightness;
        Weather.reload();

        if (GlobalConfig.utilities.vpn.enabled)
            VPN;
    }
}

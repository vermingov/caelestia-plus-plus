//@ pragma Env QS_CRASHREPORT_URL=https://github.com/caelestia-dots/shell/issues/new?template=crash.yml
//@ pragma DefaultEnv QS_NO_RELOAD_POPUP=1
//@ pragma DefaultEnv QS_DROP_EXPENSIVE_FONTS=1
//@ pragma DefaultEnv QSG_RENDER_LOOP=threaded
//@ pragma DefaultEnv QT_QUICK_FLICKABLE_WHEEL_DECELERATION=10000

import "modules"
import "modules/drawers"
import "modules/background"
import "modules/areapicker"
import "modules/easteregg"
import "modules/firewall"
import "modules/protection"
import "modules/features"
import "modules/debug"
import QtQuick
import Quickshell
import qs.services

ShellRoot {
    id: root

    settings.watchFiles: false

    Binding {
        target: ShellState
        property: "shellRoot"
        value: root
    }

    GSFLoader {}
    ServiceLoader {}

    Background {}
    Drawers {}
    AreaPicker {}
    FirewallPrompt {}
    ProtectionPrompt {}
    SecurityCenter {}
    FeaturesMenu {}
    DebugPanel {}
    SetupPrompt {}

    ConfigToasts {}
    Shortcuts {}
    EasterEgg {}
    IsraelEgg {}
    BatteryMonitor {}

    // Nothing below is needed for the first frame. Once the bar is up, the
    // lock screen and idle monitors load, then the settings module (the
    // largest tree in the shell) is compiled so its first open is instant.
    // Each step is a short synchronous compile a couple of seconds after
    // startup: the asynchronous type loader takes many times longer over
    // quickshell's qs: scheme and stalls any synchronous load that needs
    // the same files meanwhile.
    property list<var> warmed

    function warm(path: string): void {
        const comp = Qt.createComponent(Qt.resolvedUrl(path));
        if (comp.status === Component.Error)
            console.warn(`warm: ${path}: ${comp.errorString()}`);
        warmed.push(comp);
    }

    Loader {
        id: lockAndIdle

        source: "modules/LockAndIdle.qml"
        active: false
    }

    Timer {
        running: true
        interval: 1500
        onTriggered: lockAndIdle.active = true
    }

    Timer {
        running: true
        interval: 2500
        onTriggered: {
            root.warm("modules/bar/popouts/NexusHost.qml");
            root.warm("modules/nexus/NexusWindow.qml");
        }
    }
}

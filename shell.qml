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
import "modules/lock"
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
    Lock {
        id: lock
    }
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
    IdleMonitors {
        lock: lock
    }
}

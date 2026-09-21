import QtQuick
import Quickshell
import Caelestia
import Caelestia.Config
import qs.services

// What the config plugin makes of shell.json and the token file, said out
// loud. cae reads the same files and says the same thing once it is the
// shell looking over the machine, so this one stands down rather than
// complain twice about one broken file.
Scope {
    id: root

    readonly property bool active: !ExternalBar.hasScan

    Connections {
        function onLoaded(): void {
            if (root.active && GlobalConfig.utilities.toasts.configLoaded)
                Toaster.toast(qsTr("Config loaded"), qsTr("Config loaded successfully!"), "rule_settings");
        }

        function onLoadFailed(error: string, screen: string): void {
            if (!root.active)
                return;
            Toaster.toast(qsTr("Failed to parse config%1").arg(screen ? " for " + screen : ""), error, "settings_alert", Toast.Warning);
        }

        function onSaveFailed(error: string, screen: string): void {
            if (!root.active)
                return;
            Toaster.toast(qsTr("Failed to save config%1").arg(screen ? " for " + screen : ""), error, "settings_alert", Toast.Error);
        }

        function onUnknownOption(key: string, screen: string): void {
            if (!root.active)
                return;
            Toaster.toast(qsTr("Unknown option in%1 config").arg(screen ? " " + screen : ""), key, "question_mark", Toast.Warning);
        }

        target: GlobalConfig
    }

    Connections {
        function onLoadFailed(error: string, screen: string): void {
            if (!root.active)
                return;
            Toaster.toast(qsTr("Failed to parse token config%1").arg(screen ? "for " + screen : ""), error, "settings_alert", Toast.Warning);
        }

        function onUnknownOption(key: string, screen: string): void {
            if (!root.active)
                return;
            Toaster.toast(qsTr("Unknown option in%1 token config").arg(screen ? " " + screen : ""), key, "question_mark", Toast.Warning);
        }

        target: TokenConfig
    }
}

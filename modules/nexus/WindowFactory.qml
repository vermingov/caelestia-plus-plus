pragma Singleton

import QtQuick
import Quickshell
import qs.services

// Shortcuts and the utilities toggles reference this singleton at startup;
// the window itself sits in NexusWindow.qml and is compiled on first use
// (or by shell.qml's warm-up), so the nexus module stays out of the
// startup compile.
Singleton {
    id: root

    property Component windowComp

    function create(parent: Item, props: var): void {
        // The settings are cae's now, where cae is the shell that is
        // running: a word down its socket, which is what `cae-shell settings`
        // is. The window below is for a machine that has not got it.
        if (ExternalBar.binary === "cae-shell" && ExternalBar.external) {
            ExternalBar.ask(["settings"]);
            return;
        }
        if (!windowComp)
            windowComp = Qt.createComponent(Qt.resolvedUrl("NexusWindow.qml"));
        if (windowComp.status === Component.Error) {
            console.error("WindowFactory:", windowComp.errorString());
            return;
        }
        windowComp.createObject(parent ?? dummy, props);
    }

    QtObject {
        id: dummy
    }
}

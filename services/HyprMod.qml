pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Bridge to the HyprMod Hyprland-customizer layer. Three surfaces, all through
// the hyprmod helper:
// curated scalar knobs from variables.lua, arbitrary option overrides on top
// of the lua config, and custom keybinds. Writes are optimistic in the UI and
// serialized through a queue so rapid changes never race the helper's
// read-modify-write.
Singleton {
    id: root

    property bool available
    property var knobs: ({})

    // Full option surface: hyprctl descriptions entries + override state
    property var schema: []
    property var overrides: ({})
    property var customBinds: []

    // Saved monitor layout: name -> {mode, position, scale, ...}, plus the
    // primary monitor name (workspace 1 pinned there)
    property var monitors: ({})
    property string primaryMonitor

    property var pendingCommands: []

    function get(key: string, fallback: var): var {
        return knobs[key] !== undefined ? knobs[key] : fallback;
    }

    function set(key: string, value: var): void {
        const updated = Object.assign({}, knobs);
        updated[key] = value;
        knobs = updated;
        enqueue(["set", key, String(value)], key);
    }

    function optionValue(option: var): var {
        if (overrides[option.name] !== undefined)
            return overrides[option.name];
        return option.current !== null ? option.current : option.default;
    }

    function setOption(name: string, value: var): void {
        const updated = Object.assign({}, overrides);
        updated[name] = value;
        overrides = updated;
        enqueue(["set-option", name, String(value)], name);
    }

    function unsetOption(name: string): void {
        const updated = Object.assign({}, overrides);
        delete updated[name];
        overrides = updated;
        enqueue(["unset-option", name], name);
    }

    function setMonitor(name: string, spec: var): void {
        const updated = Object.assign({}, monitors);
        updated[name] = spec;
        monitors = updated;
        enqueue(["set-monitor", name, JSON.stringify(spec)], "monitor:" + name);
    }

    function delMonitor(name: string): void {
        const updated = Object.assign({}, monitors);
        delete updated[name];
        monitors = updated;
        enqueue(["del-monitor", name], "monitor:" + name);
    }

    function setPrimary(name: string): void {
        primaryMonitor = name;
        enqueue(["set-primary", name], "primary");
    }

    function addBind(combo: string, kind: string, value: string, flags: string): void {
        enqueue(["add-bind", combo, kind, value, flags], null);
    }

    function delBind(index: int): void {
        enqueue(["del-bind", String(index)], null);
    }

    function refreshSchema(): void {
        schemaProc.running = true;
        overridesProc.running = true;
    }

    function refreshState(): void {
        overridesProc.running = true;
    }

    // collapseKey: queued commands with the same non-null key are superseded
    // by the newest one (slider spam); bind edits always run in order.
    function enqueue(command: var, collapseKey: var): void {
        if (collapseKey !== null)
            pendingCommands = pendingCommands.filter(entry => entry.key !== collapseKey);
        pendingCommands = pendingCommands.concat([{
            key: collapseKey,
            command
        }]);
        runQueue();
    }

    function runQueue(): void {
        if (ctlProc.running || !pendingCommands.length || !Helpers.ready)
            return;
        const entry = pendingCommands[0];
        pendingCommands = pendingCommands.slice(1);
        ctlProc.command = Helpers.command("hyprmod", entry.command);
        ctlProc.running = true;
    }

    // Nothing is sent before the helper is resolved, so whatever the UI piled
    // up in the meantime goes out the moment it is.
    Connections {
        target: Helpers

        function onReadyChanged(): void {
            root.runQueue();
        }
    }

    Process {
        id: dumpProc

        running: Helpers.ready
        command: Helpers.command("hyprmod", ["dump"])

        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    root.knobs = JSON.parse(text);
                    root.available = Object.keys(root.knobs).length > 0;
                } catch (e) {
                    root.available = false;
                }
            }
        }
    }

    Process {
        id: schemaProc

        command: Helpers.command("hyprmod", ["schema"])

        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    root.schema = JSON.parse(text);
                } catch (e) {
                }
            }
        }
    }

    Process {
        id: overridesProc

        command: Helpers.command("hyprmod", ["overrides"])

        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const state = JSON.parse(text);
                    root.overrides = state.options;
                    root.customBinds = state.binds;
                    root.monitors = state.monitors ?? {};
                    root.primaryMonitor = state.primary ?? "";
                } catch (e) {
                }
            }
        }
    }

    Process {
        id: ctlProc

        onExited: {
            root.runQueue();
            // Bind list mutations come back from the helper's state file
            if (!ctlProc.running)
                overridesProc.running = true;
        }
    }
}

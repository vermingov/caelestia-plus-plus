pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.utils

// Per-app GPU assignments. Maps desktop entry ids to a GPU (by PCI slot,
// stable across reboots unlike render node numbers) and injects the matching
// render-offload env vars when the launcher starts the app. Persisted to
// state and hot-reloaded, same pattern as ShellPrefs.
Singleton {
    id: root

    readonly property string statePath: `${Paths.state}/app-gpus.json`

    // [{name, driver, slot, node}] — one per /dev/dri render node
    property list<var> gpus: []
    readonly property bool multiGpu: gpus.length > 1

    // desktop entry id -> PCI slot
    property var assignments: ({})

    function gpuFor(appId: string): var {
        const slot = assignments[appId];
        return slot ? gpus.find(g => g.slot === slot) ?? null : null;
    }

    function setGpu(appId: string, slot: string): void {
        const updated = Object.assign({}, assignments);
        if (slot)
            updated[appId] = slot;
        else
            delete updated[appId];
        assignments = updated;
        store.setText(JSON.stringify(assignments, null, 2) + "\n");
    }

    // Environment for launching appId on its assigned GPU. Mesa drivers take
    // DRI_PRIME=pci-<slot>; the NVIDIA proprietary driver needs the GLX/Vulkan
    // offload variables instead.
    function launchEnv(appId: string): var {
        const gpu = gpuFor(appId);
        if (!gpu)
            return null;
        if (gpu.driver === "nvidia")
            return {
                __NV_PRIME_RENDER_OFFLOAD: "1",
                __GLX_VENDOR_LIBRARY_NAME: "nvidia",
                __VK_LAYER_NV_optimus: "NVIDIA_only"
            };
        return {
            DRI_PRIME: "pci-" + gpu.slot.replace(/[:.]/g, "_")
        };
    }

    Process {
        running: true
        command: ["python3", Quickshell.shellPath("assets/list-gpus.py")]

        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    root.gpus = JSON.parse(text);
                } catch (e) {
                }
            }
        }
    }

    FileView {
        id: store

        path: root.statePath
        watchChanges: true
        printErrors: false

        onLoaded: {
            try {
                root.assignments = JSON.parse(text());
            } catch (e) {
            }
        }
        onFileChanged: reload()
    }
}

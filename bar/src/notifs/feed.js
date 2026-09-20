import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { onMounted, ref, shallowRef } from "vue";

/**
 * What the server knows, kept current.
 *
 * The server pushes the whole feed on every change, so there is nothing to
 * merge and nothing to get out of step: whatever arrived last is the truth.
 * The opening snapshot only fills in until the first push, and must not
 * overwrite one that has already landed.
 */
export function useFeed() {
    const feed = shallowRef({ list: [], dnd: false, centre: "" });
    const config = shallowRef({
        openExpanded: false,
        actionOnClick: false,
        groupPreviewNum: 3,
        clearThreshold: 0.3,
        expandThreshold: 20
    });
    // Which output this surface is on, so it knows whether the centre was
    // opened here or on another screen.
    const output = ref("");

    onMounted(async () => {
        let pushed = false;
        await listen("notifs", event => {
            pushed = true;
            feed.value = event.payload;
        });
        // shell.json changed underneath us; the bar's own watcher says so.
        await listen("config", async () => (config.value = await invoke("notifs_config")));

        output.value = await invoke("monitor");
        config.value = await invoke("notifs_config");
        const snapshot = await invoke("notifs");
        if (!pushed) feed.value = snapshot;
        invoke("notifs_log", {
            message: `ready on ${output.value || "an unnamed output"}, ${snapshot.list.length} in the list, ${window.innerWidth}x${window.innerHeight}`
        });
    });

    return { feed, config, output };
}

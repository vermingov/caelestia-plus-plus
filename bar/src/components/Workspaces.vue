<script setup>
import { invoke } from "@tauri-apps/api/core";
import { computed, nextTick, ref, watch } from "vue";

const props = defineProps({
    workspaces: { type: Array, default: () => [] },
    // The shell's own bar options: a fixed group that pages rather than a row
    // whose length changes under the pointer every time a workspace empties,
    // plus the three looks the config can ask for.
    options: {
        type: Object,
        default: () => ({
            shown: 5,
            occupiedBg: true,
            activeTrail: true,
            showWindows: false,
            label: "",
            occupiedLabel: "",
            activeLabel: ""
        })
    }
});

const shown = computed(() => Math.max(1, props.options.shown));

const focused = computed(() => props.workspaces.find(workspace => workspace.focused)?.id ?? 1);

// The group the focused workspace falls in: 1–5, then 6–10, and so on.
const offset = computed(() => Math.floor((focused.value - 1) / shown.value) * shown.value);

const pips = computed(() =>
    Array.from({ length: shown.value }, (_, index) => {
        const id = offset.value + index + 1;
        const workspace = props.workspaces.find(entry => entry.id === id);
        const occupied = (workspace?.windows ?? 0) > 0;
        const isFocused = id === focused.value;

        // The config can put a character in place of the number — one for
        // every pip, and separate ones for the occupied and focused states.
        const label =
            (isFocused && props.options.activeLabel) ||
            (occupied && props.options.occupiedLabel) ||
            props.options.label ||
            String(id);

        return { id, occupied, focused: isFocused, label, windows: workspace?.windows ?? 0 };
    })
);

// A filled track behind the run of occupied pips, as the shell draws it: it
// says which part of the row has anything on it without marking each one.
const occupiedRun = computed(() => {
    const occupied = pips.value.map(pip => pip.occupied);
    const first = occupied.indexOf(true);
    if (first === -1) return null;
    const last = occupied.lastIndexOf(true);
    return { first, count: last - first + 1 };
});

const track = ref(null);
// The marker is placed from the focused pip's real box rather than from a
// count of pips, so a two-digit workspace does not push it out of register.
const marker = ref({ x: 0, width: 0, shown: false });

// The trail: while the marker travels it stretches to cover the ground
// between where it was and where it is going, then settles. Without it the
// jump between distant workspaces reads as a teleport.
function syncMarker() {
    const current = track.value?.querySelector(".ws.focused");
    if (!current) {
        marker.value = { ...marker.value, shown: false };
        return;
    }

    const to = { x: current.offsetLeft, width: current.offsetWidth };
    if (props.options.activeTrail && marker.value.shown) {
        const from = marker.value;
        const left = Math.min(from.x, to.x);
        const right = Math.max(from.x + from.width, to.x + to.width);
        marker.value = { x: left, width: right - left, shown: true };
        // Settle onto the destination on the next frame, so the stretch and
        // the collapse are two steps of one movement.
        requestAnimationFrame(() => requestAnimationFrame(() => {
            marker.value = { ...to, shown: true };
        }));
        return;
    }
    marker.value = { ...to, shown: true };
}

watch(pips, () => nextTick(syncMarker), { immediate: true, deep: true });

// Scrolling the row walks the workspaces, the way it does in the shell's bar.
function onWheel(event) {
    invoke("cycle_workspace", { forward: event.deltaY > 0 });
}
</script>

<template>
    <div ref="track" class="workspaces" @wheel.prevent.stop="onWheel">
        <div
            class="marker"
            :style="{
                transform: `translateX(${marker.x}px)`,
                width: `${marker.width}px`,
                opacity: marker.shown ? 1 : 0
            }"
        ></div>

        <!-- Behind the pips, covering the run that has windows on it. -->
        <div
            v-if="options.occupiedBg && occupiedRun"
            class="occupied-bg"
            :style="{
                left: `${3 + occupiedRun.first * 24}px`,
                width: `${occupiedRun.count * 24 - 2}px`
            }"
        ></div>

        <div
            v-for="pip in pips"
            :key="pip.id"
            class="ws"
            :class="{ focused: pip.focused, occupied: pip.occupied }"
            @click="invoke('focus_workspace', { id: pip.id })"
        >
            {{ pip.label }}
            <span v-if="options.showWindows && pip.windows" class="count">{{ pip.windows }}</span>
        </div>
    </div>
</template>

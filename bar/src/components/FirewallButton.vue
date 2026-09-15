<script setup>
import { invoke } from "@tauri-apps/api/core";
import { computed, inject } from "vue";

const props = defineProps({
    guards: { type: Object, required: true }
});

const popout = inject("popout");

// Neither guard up is a struck shield, up but nothing waiting is a calm one,
// something waiting is the one that asks to be looked at.
const glyph = computed(() => {
    if (!props.guards.connected) return "gpp_bad";
    if (props.guards.pending > 0) return "gpp_maybe";
    return "gpp_good";
});

// Jump to whichever guard is asking; otherwise land on the overview.
function openSecurity() {
    invoke("security", { tab: props.guards.pending > 0 ? "firewall" : "overview" });
}
</script>

<template>
    <div
        class="pill button icon-only"
        :class="{ dim: !guards.connected, lit: guards.pending > 0, pulsing: guards.pending > 0 }"
        @click="openSecurity"
        @mouseenter="popout.open('guards', $event)"
        @mouseleave="popout.close()"
    >
        <Transition name="swap" mode="out-in">
            <span :key="glyph" class="glyph material-symbols-rounded">{{ glyph }}</span>
        </Transition>
        <!-- The count, where it cannot be missed and only while there is one. -->
        <Transition name="badge">
            <span v-if="guards.pending > 0" class="badge">{{ guards.pending }}</span>
        </Transition>
    </div>
</template>

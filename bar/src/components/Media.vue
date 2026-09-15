<script setup>
import { invoke } from "@tauri-apps/api/core";
import { computed, inject } from "vue";

const props = defineProps({
    playing: { type: Object, default: null }
});

const popout = inject("popout");

// Title first, artist after: on a bar with one line, the title is the thing
// being recognised and the artist is the confirmation.
const label = computed(() => props.playing?.title || props.playing?.identity || "");
</script>

<template>
    <div
        v-if="playing && label"
        class="media"
        @click="invoke('media_control', { action: 'PlayPause' })"
        @click.middle="invoke('media_control', { action: 'Next' })"
        @wheel.prevent="invoke('media_control', { action: $event.deltaY > 0 ? 'Next' : 'Previous' })"
        @mouseenter="popout.open('media', $event)"
        @mouseleave="popout.close()"
    >
        <span class="glyph material-symbols-rounded">{{ playing.playing ? "graphic_eq" : "pause" }}</span>
        <Transition name="swap" mode="out-in">
            <span :key="label" class="label">{{ label }}</span>
        </Transition>
    </div>
</template>

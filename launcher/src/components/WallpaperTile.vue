<script setup>
import { convertFileSrc } from "@tauri-apps/api/core";
import { computed } from "vue";

const props = defineProps({
    entry: { type: Object, required: true },
    current: { type: Boolean, default: false }
});

// A cached thumbnail where the CLI has made one, the original otherwise — a
// grid of full-size wallpapers would have the webview decoding tens of
// megabytes to draw a few hundred pixels.
const src = computed(() => (props.entry.preview ? convertFileSrc(props.entry.preview) : ""));
</script>

<template>
    <figure class="tile" :class="{ current }">
        <img v-if="src" :src="src" alt="" draggable="false" loading="lazy" />
        <figcaption>
            <span class="name">{{ entry.name }}</span>
            <span v-if="entry.category" class="category">{{ entry.category }}</span>
        </figcaption>
        <span v-if="entry.marked" class="badge material-symbols-rounded">check</span>
    </figure>
</template>

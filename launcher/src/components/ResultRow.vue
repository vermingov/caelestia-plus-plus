<script setup>
import { convertFileSrc } from "@tauri-apps/api/core";
import { computed } from "vue";

const props = defineProps({
    entry: { type: Object, required: true },
    current: { type: Boolean, default: false }
});

// Icons and previews are absolute paths the backend resolved; the webview can
// only load them through Tauri's asset protocol.
const iconSrc = computed(() => (props.entry.icon ? convertFileSrc(props.entry.icon) : ""));
</script>

<template>
    <div class="row" :class="{ current }">
        <img v-if="iconSrc" class="icon" :src="iconSrc" alt="" draggable="false" />
        <span v-else-if="entry.glyph" class="icon glyph material-symbols-rounded">{{ entry.glyph }}</span>
        <span v-else-if="entry.swatches.length" class="icon swatches">
            <i v-for="(colour, i) in entry.swatches.slice(0, 4)" :key="i" :style="{ background: `#${colour}` }"></i>
        </span>
        <div v-else class="icon missing"></div>

        <span class="name" :class="{ error: entry.trailing === 'Calculator' && entry.marked }">{{ entry.name }}</span>
        <span class="comment">{{ entry.comment }}</span>

        <!-- The heart on a favourite, the tick on whatever is already in use. -->
        <span v-if="entry.marked && entry.trailing === 'Application'" class="mark material-symbols-rounded">favorite</span>
        <span v-else-if="entry.marked && entry.trailing !== 'Calculator'" class="mark material-symbols-rounded">check</span>

        <span class="kind">{{ entry.trailing }}</span>
    </div>
</template>

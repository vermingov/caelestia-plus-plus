<script setup>
import { convertFileSrc } from "@tauri-apps/api/core";
import { computed } from "vue";

const props = defineProps({
    entry: { type: Object, required: true },
    current: { type: Boolean, default: false }
});

// The icon is an absolute path the backend resolved; the webview can only
// load it through Tauri's asset protocol.
const iconSrc = computed(() => (props.entry.icon ? convertFileSrc(props.entry.icon) : ""));
</script>

<template>
    <div class="row" :class="{ current }">
        <img v-if="iconSrc" class="icon" :src="iconSrc" :alt="''" draggable="false" />
        <div v-else class="icon missing"></div>
        <span class="name">{{ entry.name }}</span>
        <span class="comment">{{ entry.comment }}</span>
        <span class="kind">Application</span>
    </div>
</template>

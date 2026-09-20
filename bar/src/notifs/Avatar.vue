<script setup>
import { convertFileSrc } from "@tauri-apps/api/core";
import { computed, ref, watch } from "vue";

import { CRITICAL, LOW, fallbackGlyph } from "./glyph.js";

const props = defineProps({
    image: { type: String, default: "" },
    appIcon: { type: String, default: "" },
    summary: { type: String, default: "" },
    urgency: { type: Number, default: 1 },
    // 0 to 100, or null when the notification is not reporting any.
    progress: { type: Number, default: null }
});

// A picture that will not load is treated as one that was never sent, so the
// slot falls through to the next best thing instead of showing a torn image.
const broken = ref({ image: false, icon: false });
watch(
    () => [props.image, props.appIcon],
    () => (broken.value = { image: false, icon: false })
);

const image = computed(() => (props.image && !broken.value.image ? convertFileSrc(props.image) : ""));
const icon = computed(() => (props.appIcon && !broken.value.icon ? convertFileSrc(props.appIcon) : ""));

// Symbolic icons are drawn in black and meant to be recoloured by the
// toolkit. Nothing recolours them here, so on a dark pane they vanish.
const symbolic = computed(() => /symbolic/.test(props.appIcon));

const tone = computed(() => (props.urgency === CRITICAL ? "critical" : props.urgency === LOW ? "low" : ""));

// The ring is a circle with all but a fraction of its outline dashed away.
const RADIUS = 20;
const LENGTH = 2 * Math.PI * RADIUS;
const dash = computed(() => `${(Math.min(100, Math.max(0, props.progress ?? 0)) / 100) * LENGTH} ${LENGTH}`);
</script>

<template>
    <div class="avatar" :class="tone">
        <img v-if="image" class="picture" :src="image" alt="" @error="broken.image = true" />
        <img
            v-else-if="icon"
            class="icon"
            :class="{ symbolic }"
            :src="icon"
            alt=""
            @error="broken.icon = true"
        />
        <span v-else class="glyph material-symbols-rounded">{{ fallbackGlyph(summary, urgency) }}</span>

        <!-- With a picture in the slot, the sender's own icon rides on its
             corner so it is still clear who is speaking. -->
        <span v-if="image && icon" class="sender">
            <img :class="{ symbolic }" :src="icon" alt="" @error="broken.icon = true" />
        </span>

        <svg v-if="progress !== null" class="ring" viewBox="0 0 44 44" aria-hidden="true">
            <circle cx="22" cy="22" :r="RADIUS" :stroke-dasharray="dash" />
        </svg>
    </div>
</template>

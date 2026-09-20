<script setup>
import { computed } from "vue";

import { openLink } from "./api.js";
import { runs } from "./markup.js";

const props = defineProps({
    body: { type: String, default: "" }
});

// Runs of plain text with their styling beside them. No markup from the body
// reaches the page as markup; see markup.js for why that matters here.
const parts = computed(() => runs(props.body));
</script>

<template>
    <p class="body">
        <template v-for="(run, index) in parts" :key="index">
            <a
                v-if="run.href"
                :class="{ bold: run.bold, italic: run.italic }"
                @click.stop.prevent="openLink(run.href)"
                >{{ run.text }}</a
            >
            <span v-else :class="{ bold: run.bold, italic: run.italic, underline: run.underline }">{{
                run.text
            }}</span>
        </template>
    </p>
</template>

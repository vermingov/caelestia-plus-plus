<script setup>
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

// The two marks the bar can draw itself, inlined rather than linked: an <img>
// cannot be recoloured, and both of these take the bar's own colour.
import caelestia from "../assets/logo.svg?raw";
import cachyos from "../assets/cachyos-rounded.svg?raw";

const marks = { caelestia, cachyos };

// Resolved by the backend: the shell config's choice, then the
// distribution's, then Caelestia's mark. Only marks the bar draws itself --
// an image off disk is not always one a webview will load, and one that fails
// is a broken-image square in the corner of the bar.
const logo = ref({
    kind: "caelestia",
    show: true
});

onMounted(async () => {
    logo.value = await invoke("logo");
});
</script>

<template>
    <!-- The first thing in the row. It is a button because it is the one
         thing on the bar everybody tries to click: it opens the launcher. -->
    <div v-if="logo.show" class="logo" @click="invoke('toggle_launcher')">
        <span v-if="marks[logo.kind]" class="mark" v-html="marks[logo.kind]"></span>
    </div>
</template>

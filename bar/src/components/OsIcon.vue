<script setup>
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

// The two marks the bar can draw itself, inlined rather than linked: an <img>
// cannot be recoloured, and both of these take the bar's own colour.
import caelestia from "../assets/logo.svg?raw";
import cachyos from "../assets/cachyos-rounded.svg?raw";

const marks = { caelestia, cachyos };

// Resolved by the backend, which follows the same order the shell does: the
// user's own image, then the shell config's choice, then the distribution's,
// then Caelestia's mark.
const logo = ref({
    kind: "caelestia",
    path: "",
    show: true,
    endcap: true,
    scale: 1,
    offsetX: 0,
    offsetY: 0
});

onMounted(async () => {
    logo.value = await invoke("logo");

    // On the document, not on this element: the pill's own surface is masked
    // with the same mark, and a custom property set here would be invisible
    // to it — a scaled logo and an unscaled cut-out.
    const root = document.documentElement;
    root.style.setProperty("--logo-scale", logo.value.scale || 1);
    root.style.setProperty("--logo-x", `${logo.value.offsetX || 0}px`);
    root.style.setProperty("--logo-y", `${logo.value.offsetY || 0}px`);

    // Only the marks the bar draws itself can be cut out of the pill. An
    // image somebody supplied is their artwork, with its own silhouette, so
    // the pill keeps a plain rounded end instead.
    document.body.classList.toggle("custom-logo", logo.value.kind === "file");
});
</script>

<template>
    <!-- The endcap. It is a button because it is the one thing on the bar
         everybody tries to click: it opens the launcher. -->
    <div
        v-if="logo.show"
        class="logo"
        :class="{ endcap: logo.endcap }"
        :style="{ transform: `translate(var(--logo-x, 0px), var(--logo-y, 0px))` }"
        @click="invoke('toggle_launcher')"
    >
        <span v-if="marks[logo.kind]" class="mark" v-html="marks[logo.kind]"></span>
        <img v-else-if="logo.path" class="mark image" :src="convertFileSrc(logo.path)" alt="" draggable="false" />
    </div>
</template>

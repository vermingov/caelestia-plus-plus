<script setup>
import { invoke } from "@tauri-apps/api/core";

defineProps({
    specials: { type: Array, default: () => [] }
});

// The four the shell's own rules put things on, plus a fallback for anything
// else somebody has bound. A named workspace deserves its own glyph — the
// point of naming it was that it holds one kind of thing.
const glyphs = {
    sysmon: "monitor_heart",
    music: "music_note",
    communication: "forum",
    todo: "checklist",
    special: "layers"
};

function glyph(name) {
    return glyphs[name] ?? glyphs.special;
}
</script>

<template>
    <!-- Only drawn while something is on one: an empty scratchpad is not a
         thing to navigate to, and a permanently-present row of dead pills is
         how a bar fills up with noise. -->
    <TransitionGroup v-if="specials.length" tag="div" name="tray" class="section specials">
        <div
            v-for="special in specials"
            :key="special.name"
            class="pill button icon-only"
            :class="{ lit: special.open }"
            :title="`${special.name} · ${special.windows} window${special.windows === 1 ? '' : 's'}`"
            @click="invoke('toggle_special', { name: special.name })"
        >
            <span class="glyph material-symbols-rounded">{{ glyph(special.name) }}</span>
        </div>
    </TransitionGroup>
</template>

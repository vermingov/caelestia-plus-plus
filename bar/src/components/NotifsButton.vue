<script setup>
import { invoke } from "@tauri-apps/api/core";
import { computed } from "vue";

const props = defineProps({
    // How many have arrived since the centre was last open, whether they are
    // silenced, and which output the centre is open on. The bar is never sent
    // the list itself.
    notifs: { type: Object, required: true },
    // The output this bar is on, so the bell lights only for its own screen.
    output: { type: String, default: "" }
});

const open = computed(() => props.notifs.centre !== "" && props.notifs.centre === props.output);
const glyph = computed(() => (props.notifs.dnd ? "notifications_off" : "notifications"));

// Counts past two digits stop being counts anybody reads.
const count = computed(() => (props.notifs.unseen > 99 ? "99+" : String(props.notifs.unseen)));
</script>

<template>
    <!-- A click opens the centre on this screen; the same click shuts it.
         Right-click silences, because that is the one thing about
         notifications worth reaching without opening anything. -->
    <div
        class="pill button icon-only"
        :class="{ lit: open, dim: notifs.dnd && !open }"
        @click="invoke('notif_centre')"
        @contextmenu.prevent="invoke('notif_dnd', { on: !notifs.dnd })"
    >
        <Transition name="swap" mode="out-in">
            <span :key="glyph" class="glyph material-symbols-rounded">{{ glyph }}</span>
        </Transition>
        <Transition name="badge">
            <span v-if="notifs.unseen > 0 && !open" class="badge">{{ count }}</span>
        </Transition>
    </div>
</template>

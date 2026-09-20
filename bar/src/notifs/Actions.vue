<script setup>
import { onBeforeUnmount, ref } from "vue";

import { act, close, copy } from "./api.js";

const props = defineProps({
    notif: { type: Object, required: true }
});

// The copy button says it worked for a moment, the way the shell's did.
const copied = ref(false);
let settle = null;

function copyBody() {
    copy(props.notif.body || props.notif.summary);
    copied.value = true;
    clearTimeout(settle);
    settle = setTimeout(() => (copied.value = false), 3000);
}

onBeforeUnmount(() => clearTimeout(settle));
</script>

<template>
    <div class="actions" @pointerdown.stop>
        <button class="round" title="Dismiss" @click.stop="close(notif.id)">
            <span class="material-symbols-rounded">close</span>
        </button>
        <button
            v-for="action in notif.actions"
            :key="action.identifier"
            class="label"
            @click.stop="act(notif.id, action.identifier)"
        >
            {{ action.text.trim() || "Open" }}
        </button>
        <button class="round" :title="copied ? 'Copied' : 'Copy text'" @click.stop="copyBody">
            <span class="material-symbols-rounded">{{ copied ? "inventory" : "content_copy" }}</span>
        </button>
    </div>
</template>

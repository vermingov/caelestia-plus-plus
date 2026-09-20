<script setup>
import { computed } from "vue";

import { ago } from "./ago.js";
import { closeApp } from "./api.js";
import Avatar from "./Avatar.vue";
import Row from "./Row.vue";
import { useSwipe } from "./swipe.js";

const props = defineProps({
    appName: { type: String, required: true },
    // Newest first.
    notifs: { type: Array, required: true },
    expanded: { type: Boolean, default: false },
    // How many are listed while the group is folded.
    previewed: { type: Number, default: 3 },
    now: { type: Number, required: true }
});

const emit = defineEmits(["toggle"]);

const newest = computed(() => props.notifs[0]);
const listed = computed(() => (props.expanded ? props.notifs : props.notifs.slice(0, props.previewed)));

// The group wears the most urgent face of anything in it, and whichever
// picture its notifications brought most recently.
const urgency = computed(() => Math.max(...props.notifs.map(n => n.urgency)));
const image = computed(() => props.notifs.find(n => n.image)?.image ?? "");
const appIcon = computed(() => props.notifs.find(n => n.appIcon)?.appIcon ?? "");

// Swiping the header throws the whole application's notifications away,
// which is what dragging the group did in the shell.
const swipe = useSwipe({
    onThrow: () => closeApp(props.appName),
    onFold: down => down !== props.expanded && emit("toggle")
});

function pressed(event) {
    if (event.button === 1) closeApp(props.appName);
    else swipe.down(event);
}
</script>

<template>
    <section class="group" :class="{ expanded }">
        <header
            class="group-head"
            :class="{ dragging: swipe.dragging.value }"
            :style="{ '--pull': `${swipe.pull.value}px` }"
            @pointerdown="pressed"
            @contextmenu.prevent="emit('toggle')"
        >
            <Avatar
                class="small"
                :image="image"
                :app-icon="appIcon"
                :summary="newest.summary"
                :urgency="urgency"
            />
            <span class="app">{{ appName || "Unknown" }}</span>
            <span class="time">{{ ago(newest.time, now) }}</span>
            <button
                class="count"
                :title="expanded ? 'Fold' : 'Unfold'"
                @pointerdown.stop
                @click.stop="emit('toggle')"
            >
                {{ notifs.length }}
                <span class="material-symbols-rounded">expand_more</span>
            </button>
        </header>

        <TransitionGroup name="row" tag="div" class="rows">
            <Row v-for="notif in listed" :key="notif.id" :notif="notif" :expanded="expanded" :now="now" />
        </TransitionGroup>
    </section>
</template>

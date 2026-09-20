<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, provide, ref, watch } from "vue";

import { useNow } from "./notifs/ago.js";
import { openCentre, reach } from "./notifs/api.js";
import Centre from "./notifs/Centre.vue";
import { useFeed } from "./notifs/feed.js";
import Toast from "./notifs/Toast.vue";

const { feed, config, output } = useFeed();
const now = useNow();

// For the drag thresholds, which every swipeable thing on the page reads.
provide("notifsConfig", config);

// More than this and the newest few are shown with a count of the rest: a
// column of toasts the height of the screen is not being read by anybody.
const MOST_TOASTS = 5;

// The centre opens on one output at a time, and this surface is one output.
const centreOpen = computed(() => feed.value.centre !== "" && feed.value.centre === output.value);

// Nothing pops up beside an open centre. The server already stops marking
// anything as a toast while it is open, so this only matters for the moment
// the two would cross: the pane is glass, and toasts leaving behind it showed
// through it.
const popups = computed(() => (centreOpen.value ? [] : feed.value.list.filter(notif => notif.popup)));
const shown = computed(() => popups.value.slice(0, MOST_TOASTS));
const waiting = computed(() => popups.value.length - shown.value.length);

const stack = ref(null);
const centre = ref(null);
const corner = ref(null);

// Where an element is laid out, whatever transform it is wearing. Toasts slide
// in and are dragged about, and the region that accepts the pointer should be
// where they belong rather than where an animation has them this frame.
function laidOut(element) {
    let x = 0;
    let y = 0;
    for (let at = element; at; at = at.offsetParent) {
        x += at.offsetLeft;
        y += at.offsetTop;
    }
    return { x, y, width: element.offsetWidth, height: element.offsetHeight };
}

// The surface runs the height of the screen and is empty nearly all of the
// time. The compositor is told exactly which boxes have something in them,
// and everywhere else the pointer goes straight through to what is underneath.
function report() {
    const boxes = [];
    for (const toast of stack.value?.querySelectorAll(".toast:not(.toast-leave-active)") ?? []) {
        boxes.push(laidOut(toast));
    }
    const panel = centre.value?.root;
    if (panel) boxes.push(laidOut(panel));
    if (corner.value) boxes.push(laidOut(corner.value));
    reach(boxes.filter(box => box.width > 0 && box.height > 0));
}

watch([shown, centreOpen], () => nextTick(report), { flush: "post" });

// A toast unfolding changes the stack's height without changing the list.
let resized = null;
onMounted(() => {
    resized = new ResizeObserver(report);
    resized.observe(stack.value);
    report();
});
onBeforeUnmount(() => resized?.disconnect());
</script>

<template>
    <!-- The top corner of the screen opens the centre, as the edge of the
         shell's sidebar did. A sliver, so it costs the windows beneath it
         nothing but their outermost pixels. -->
    <div v-if="!centreOpen" ref="corner" class="corner" @mouseenter="openCentre(true)"></div>

    <div ref="stack" class="stack">
        <TransitionGroup name="toast">
            <Toast v-for="notif in shown" :key="notif.id" :notif="notif" :config="config" :now="now" />
        </TransitionGroup>
        <Transition name="swap">
            <p v-if="waiting > 0" class="waiting">{{ waiting }} more waiting</p>
        </Transition>
    </div>

    <Transition name="centre" @after-enter="report" @after-leave="report">
        <Centre v-if="centreOpen" ref="centre" :feed="feed" :config="config" :now="now" />
    </Transition>
</template>

<script setup>
import { computed, onBeforeUnmount, ref } from "vue";

import { whenPointerLeaves } from "../leaving.js";
import Actions from "./Actions.vue";
import { ago } from "./ago.js";
import { act, close, dismiss, hold, pointer } from "./api.js";
import Avatar from "./Avatar.vue";
import Body from "./Body.vue";
import { CRITICAL } from "./glyph.js";
import { plain } from "./markup.js";
import { useSwipe } from "./swipe.js";

const props = defineProps({
    notif: { type: Object, required: true },
    config: { type: Object, required: true },
    now: { type: Number, required: true }
});

const root = ref(null);
const expanded = ref(props.config.openExpanded);

const preview = computed(() => plain(props.notif.body));
const critical = computed(() => props.notif.urgency === CRITICAL);

const swipe = useSwipe({
    onThrow: () => dismiss(props.notif.id),
    onFold: down => (expanded.value = down)
});

// Thrown left, it leaves to the left. Anything else leaves the way it came.
const exit = computed(() => (swipe.pull.value < 0 ? "calc(-100% - 80px)" : null));

// A toast under the pointer keeps its place: its clock stops when the pointer
// arrives and starts again, from the top, when it goes.
//
// Arriving is an event the page can trust. Going is not — the pointer leaves
// the surface's input region and no `mouseleave` follows — so while a toast
// is held the compositor is asked where the pointer really is.
let stopWatching = null;

function held() {
    if (stopWatching) return;
    hold(props.notif.id, true);
    stopWatching = whenPointerLeaves(
        () => [root.value?.getBoundingClientRect()],
        () => {
            if (swipe.dragging.value) return;
            released();
        },
        { locate: pointer }
    );
}

function released() {
    if (!stopWatching) return;
    stopWatching();
    stopWatching = null;
    hold(props.notif.id, false);
}

onBeforeUnmount(() => stopWatching?.());

function clicked(event) {
    if (event.button !== 0 || !swipe.wasClick()) return;
    if (props.config.actionOnClick && props.notif.actions.length === 1) {
        act(props.notif.id, props.notif.actions[0].identifier);
    }
}

// The middle button throws it away outright, as it did in the shell.
function pressed(event) {
    if (event.button === 1) close(props.notif.id);
    else swipe.down(event);
}
</script>

<template>
    <article
        ref="root"
        class="toast"
        :class="{ critical, expanded, dragging: swipe.dragging.value }"
        :style="{ '--pull': `${swipe.pull.value}px`, '--exit': exit }"
        @mouseenter="held"
        @pointerdown="pressed"
        @click="clicked"
    >
        <Avatar
            :image="notif.image"
            :app-icon="notif.appIcon"
            :summary="notif.summary"
            :urgency="notif.urgency"
            :progress="notif.progress"
        />

        <div class="content">
            <!-- Who sent it is only worth a line once there is room for it. -->
            <div class="fold" :class="{ open: expanded }">
                <div class="fold-inner sender-name">{{ notif.appName }}</div>
            </div>

            <div class="headline">
                <span class="summary">{{ notif.summary }}</span>
                <span class="time">{{ ago(notif.time, now) }}</span>
            </div>

            <div v-if="preview" class="fold" :class="{ open: !expanded }">
                <div class="fold-inner preview">{{ preview }}</div>
            </div>

            <div class="fold" :class="{ open: expanded }">
                <div class="fold-inner">
                    <Body v-if="notif.body" :body="notif.body" />
                    <Actions :notif="notif" />
                </div>
            </div>
        </div>

        <button
            class="chevron"
            :title="expanded ? 'Show less' : 'Show more'"
            @pointerdown.stop
            @click.stop="expanded = !expanded"
        >
            <span class="material-symbols-rounded">expand_more</span>
        </button>
    </article>
</template>

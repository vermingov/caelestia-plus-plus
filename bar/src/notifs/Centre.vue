<script setup>
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { clear, pointer, setDnd, shutCentre } from "./api.js";
import Group from "./Group.vue";

const props = defineProps({
    feed: { type: Object, required: true },
    config: { type: Object, required: true },
    now: { type: Number, required: true }
});

const root = ref(null);
defineExpose({ root });

// One group per application, in the order each last spoke. The list arrives
// newest first, so the first time an application is seen is its newest.
const groups = computed(() => {
    const byApp = new Map();
    for (const notif of props.feed.list) {
        if (!byApp.has(notif.appName)) byApp.set(notif.appName, []);
        byApp.get(notif.appName).push(notif);
    }
    return [...byApp].map(([appName, notifs]) => ({ appName, notifs }));
});

const count = computed(() => props.feed.list.length);
const title = computed(() => {
    if (count.value === 0) return "Notifications";
    return `${count.value} notification${count.value === 1 ? "" : "s"}`;
});

// Which groups are unfolded. Kept here rather than in each group so that a
// group redrawn because its notifications changed stays as it was left.
const unfolded = ref(new Set());

function toggle(appName) {
    const next = new Set(unfolded.value);
    if (!next.delete(appName)) next.add(appName);
    unfolded.value = next;
}

// The centre shuts itself once the pointer has been in it and then gone.
//
// "Has been in it" matters: opened from a keybind, the pointer may be on the
// other side of the screen, and a panel that closed because the pointer was
// not already on it would never be seen. And "gone" is asked of the
// compositor, because leaving this surface's input region is not an event
// the page receives.
let watching = null;

onMounted(() => {
    let entered = false;
    let misses = 0;
    watching = setInterval(async () => {
        const position = await pointer();
        const box = root.value?.getBoundingClientRect();
        if (!position || !box) return;

        const [x, y] = position;
        const slack = 14;
        const inside =
            x >= box.left - slack && x <= box.right + slack && y >= box.top - slack && y <= box.bottom + slack;
        if (inside) {
            entered = true;
            misses = 0;
        } else if (entered && ++misses >= 3) {
            shutCentre();
        }
    }, 200);
});

onBeforeUnmount(() => clearInterval(watching));
</script>

<template>
    <aside ref="root" class="centre">
        <header class="centre-head">
            <h1>{{ title }}</h1>
            <button
                class="round"
                :class="{ on: feed.dnd }"
                :title="feed.dnd ? 'Turn off do not disturb' : 'Do not disturb'"
                @click="setDnd(!feed.dnd)"
            >
                <span class="material-symbols-rounded">{{
                    feed.dnd ? "do_not_disturb_on" : "do_not_disturb_off"
                }}</span>
            </button>
            <button v-if="count > 0" class="round" title="Clear all" @click="clear()">
                <span class="material-symbols-rounded">clear_all</span>
            </button>
        </header>

        <p v-if="feed.dnd" class="note">Do not disturb is on. New notifications land here without popping up.</p>

        <TransitionGroup v-if="count > 0" name="group" tag="div" class="groups">
            <Group
                v-for="group in groups"
                :key="group.appName"
                :app-name="group.appName"
                :notifs="group.notifs"
                :expanded="unfolded.has(group.appName)"
                :previewed="config.groupPreviewNum"
                :now="now"
                @toggle="toggle(group.appName)"
            />
        </TransitionGroup>
        <p v-else class="empty">All caught up</p>
    </aside>
</template>

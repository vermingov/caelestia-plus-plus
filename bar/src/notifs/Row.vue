<script setup>
import { computed } from "vue";

import Actions from "./Actions.vue";
import { ago } from "./ago.js";
import { close } from "./api.js";
import Body from "./Body.vue";
import { CRITICAL } from "./glyph.js";
import { plain } from "./markup.js";
import { useSwipe } from "./swipe.js";

const props = defineProps({
    notif: { type: Object, required: true },
    // Folded, it is one line; unfolded, it is the whole notification.
    expanded: { type: Boolean, default: false },
    now: { type: Number, required: true }
});

const excerpt = computed(() => plain(props.notif.body));
const critical = computed(() => props.notif.urgency === CRITICAL);

const swipe = useSwipe({ onThrow: () => close(props.notif.id) });

function pressed(event) {
    if (event.button === 1) close(props.notif.id);
    else swipe.down(event);
}
</script>

<template>
    <div
        class="row"
        :class="{ expanded, critical, dragging: swipe.dragging.value }"
        :style="{ '--pull': `${swipe.pull.value}px` }"
        @pointerdown="pressed"
    >
        <div class="line">
            <span class="summary">{{ notif.summary }}</span>
            <span v-if="!expanded && excerpt" class="excerpt">{{ excerpt }}</span>
            <span v-if="expanded" class="time">{{ ago(notif.time, now) }}</span>
        </div>

        <div class="fold" :class="{ open: expanded }">
            <div class="fold-inner">
                <Body v-if="notif.body" :body="notif.body" />
                <Actions :notif="notif" />
            </div>
        </div>
    </div>
</template>

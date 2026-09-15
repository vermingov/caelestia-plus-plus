<script setup>
import { inject, onMounted, onUnmounted, ref } from "vue";

const popout = inject("popout");

const time = ref("");
const date = ref("");

// Aligned to the minute rather than ticking every second: the clock shows
// minutes, so a second of work per second is fifty-nine wasted.
let timer = null;

function render() {
    const now = new Date();
    time.value = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
    date.value = now.toLocaleDateString([], { weekday: "short", day: "numeric" });

    const untilNextMinute = 60_000 - (now.getSeconds() * 1000 + now.getMilliseconds());
    timer = setTimeout(render, untilNextMinute + 20);
}

onMounted(render);
onUnmounted(() => clearTimeout(timer));
</script>

<template>
    <div
        class="section clock"
        @mouseenter="popout.open('clock', $event)"
        @mouseleave="popout.close()"
    >
        <span class="glyph material-symbols-rounded accent">calendar_month</span>
        <span class="date">{{ date }}</span>
        <span class="divider"></span>
        <!-- Keyed on the value so each new minute fades in rather than
             replacing the last one mid-glance. -->
        <Transition name="swap" mode="out-in">
            <span :key="time" class="time">{{ time }}</span>
        </Transition>
    </div>
</template>

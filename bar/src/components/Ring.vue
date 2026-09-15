<script setup>
import { computed } from "vue";

const props = defineProps({
    label: { type: String, required: true },
    // 0–100.
    value: { type: Number, default: 0 }
});

// A circle drawn as a stroked arc: the dash pattern is the reading, so the
// only thing that animates is one number and the browser composites the rest.
const RADIUS = 7;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

const dash = computed(() => {
    const fraction = Math.min(Math.max(props.value, 0), 100) / 100;
    return `${CIRCUMFERENCE * fraction} ${CIRCUMFERENCE}`;
});
</script>

<template>
    <div class="ring">
        <svg viewBox="0 0 18 18" aria-hidden="true">
            <!-- Starts at twelve o'clock and fills clockwise, which is the
                 only direction anybody reads a dial. -->
            <circle class="track" cx="9" cy="9" :r="RADIUS" />
            <circle class="fill" cx="9" cy="9" :r="RADIUS" :stroke-dasharray="dash" />
        </svg>
        <span class="name">{{ label }}</span>
        <span class="value">{{ Math.round(value) }}%</span>
    </div>
</template>

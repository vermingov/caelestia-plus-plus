<script setup>
import { onMounted, ref, watch } from "vue";

const props = defineProps({
    // One level per bar, 0–255: quantised on the way over, because a float
    // spells out as nine JSON characters where a byte spells out as three.
    bars: { type: Array, default: () => [] },
    live: { type: Boolean, default: false }
});

// A canvas rather than one element per band.
//
// Thirty frames a second of transform on twenty-eight composited nodes cost
// about a tenth of a core; the same drawing into a 100x16 canvas is one
// repaint of a small surface and costs a fraction of that.
const canvas = ref(null);
// The full width of the screen, measured on mount: the spectrum runs the
// length of the bar and sits behind everything drawn on it.
const width = ref(1920);
// The pill's height, matching --height in the stylesheet.
// The pill's height, and it has to stay in step with --height in the
// stylesheet: the canvas is positioned from the top of the bar, so anything
// taller than the pill hangs out of the bottom of it.
const HEIGHT = 38;

/// Thin bars with air between them, repeated across the whole width — the
/// shape the shell's own visualiser has. A band wide enough to read
/// individually stops being a spectrum and starts being a bar chart.
const BAND = 2;
const GAP = 3;

let context = null;
let scale = 1;

function draw() {
    if (!context || !props.bars.length) return;

    context.clearRect(0, 0, width.value, HEIGHT);
    if (!props.live) return;

    // Behind the bar's own content, so it is a texture rather than a thing to
    // read: loud enough to notice moving, quiet enough that the clock on top
    // of it stays legible.
    context.globalAlpha = 0.14;
    context.fillStyle = "#fff";

    // As many thin bands as the width takes, with the spectrum repeated and
    // mirrored across them: twenty-eight bands stretched over 1920px would be
    // fence posts, and a single unmirrored sweep leaves half the bar dead.
    const pitch = BAND + GAP;
    const bands = Math.floor(width.value / pitch);
    const levels = props.bars;
    const cycle = levels.length * 2;

    for (let i = 0; i < bands; i++) {
        // Walk the spectrum out and back, so bass meets bass where the
        // pattern repeats rather than cutting from treble to bass.
        const step = i % cycle;
        const level = levels[step < levels.length ? step : cycle - 1 - step] / 255;
        const height = Math.max(1, level * HEIGHT * 0.75);
        // Plain rectangles, not rounded ones. A radius of one on a band two
        // pixels wide softens a single row nobody can see, and building and
        // filling a path per band cost the web process over a third more than
        // four hundred rectangles the canvas can batch.
        context.fillRect(i * pitch, HEIGHT - height, BAND, height);
    }
}

watch(() => props.bars, draw);
watch(() => props.live, draw);

onMounted(() => {
    // Drawn at the display's own resolution, or every band is a blurred
    // rectangle on a HiDPI screen.
    scale = window.devicePixelRatio || 1;
    width.value = window.innerWidth;
    canvas.value.width = width.value * scale;
    canvas.value.height = HEIGHT * scale;
    context = canvas.value.getContext("2d");
    context.scale(scale, scale);
    draw();
});
</script>

<template>
    <!-- Behind the whole bar, at low opacity: the shell's own visualiser is a
         texture across the pill rather than a widget in a slot, and this is
         the same idea. -->
    <canvas ref="canvas" class="visualiser" :style="{ height: `${HEIGHT}px` }"></canvas>
</template>

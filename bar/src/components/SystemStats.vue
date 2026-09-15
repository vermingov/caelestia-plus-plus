<script setup>
import { inject } from "vue";

import Ring from "./Ring.vue";

defineProps({
    snapshot: { type: Object, required: true },
    // Which dials to draw. The fork keeps these in its preferences, and a
    // metric switched off there is switched off here.
    metrics: { type: Object, default: () => ({ cpu: true, ram: true, gpu: true }) }
});

const popout = inject("popout");
</script>

<template>
    <!-- One section, three dials. Grouping them says they are the same kind
         of thing, which is what stops the right-hand side reading as a row of
         unrelated glyphs. -->
    <div class="section">
        <div
            v-if="metrics.cpu"
            class="stat"
            @mouseenter="popout.open('cpu', $event)"
            @mouseleave="popout.close()"
        >
            <Ring label="CPU" :value="snapshot.cpu" />
        </div>

        <div
            v-if="metrics.ram"
            class="stat"
            @mouseenter="popout.open('memory', $event)"
            @mouseleave="popout.close()"
        >
            <Ring label="RAM" :value="snapshot.memory" />
        </div>

        <!-- Only where the driver reports it: a GPU dial stuck at zero is
             worse than no GPU dial. -->
        <div
            v-if="metrics.gpu && snapshot.gpu !== null"
            class="stat"
            @mouseenter="popout.open('gpu', $event)"
            @mouseleave="popout.close()"
        >
            <Ring label="GPU" :value="snapshot.gpu ?? 0" />
        </div>
    </div>
</template>

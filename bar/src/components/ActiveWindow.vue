<script setup>
import { computed, inject } from "vue";

const props = defineProps({
    active: { type: Object, default: () => ({ title: "", class: "" }) }
});

const popout = inject("popout");

// The class is the application, the title is what it is doing. Both, in that
// order, is more useful than either — and an empty desktop says so rather
// than leaving a hole in the row.
const name = computed(() => props.active.class || "Desktop");
const detail = computed(() => (props.active.title === props.active.class ? "" : props.active.title));
</script>

<template>
    <div class="active" @mouseenter="popout.open('active', $event)" @mouseleave="popout.close()">
        <!-- Keyed on the class so a switch between applications replays the
             fade rather than swapping the text in place. -->
        <Transition name="swap" mode="out-in">
            <span :key="name" class="class">{{ name }}</span>
        </Transition>
        <span v-if="detail" class="title">{{ detail }}</span>
    </div>
</template>

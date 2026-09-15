<script setup>
import { invoke } from "@tauri-apps/api/core";
import { computed, inject } from "vue";

const props = defineProps({
    features: { type: Array, default: () => [] }
});

const popout = inject("popout");

const active = computed(() => props.features.filter(feature => feature.enabled).length);
</script>

<template>
    <!-- The wrench is only drawn when there are modes to reach: on a machine
         with none, an always-dead button is worse than no button. -->
    <div
        v-if="features.length"
        class="pill button icon-only"
        :class="{ lit: active > 0 }"
        @click="invoke('features_menu')"
        @mouseenter="popout.open('features', $event)"
        @mouseleave="popout.close()"
    >
        <span class="glyph material-symbols-rounded">build</span>
        <Transition name="badge">
            <span v-if="active > 0" class="badge">{{ active }}</span>
        </Transition>
    </div>
</template>

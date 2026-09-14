<script setup>
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";

import ResultRow from "./components/ResultRow.vue";
import WallpaperTile from "./components/WallpaperTile.vue";

const query = ref("");
const results = ref({ mode: "apps", label: "Applications", action: "Open", maxShown: 8, entries: [] });
const selected = ref(0);
const field = ref(null);
const list = ref(null);

const pane = ref(null);
const entries = computed(() => results.value.entries);
const current = computed(() => entries.value[selected.value]);
const isWallpapers = computed(() => results.value.mode === "wallpapers");
const isCalc = computed(() => results.value.mode === "calc");

// Every keystroke re-ranks in the backend. No debounce: ranking is a few
// hundred microseconds and a launcher that lags the keyboard is the one thing
// it may not do. The calculator is the exception — it shells out to qalc, so
// it waits for a pause rather than spawning a process per character.
let pending = null;
async function refresh() {
    const asked = query.value;
    const answer = await invoke("search", { query: asked });
    // A slower earlier request must not overwrite a newer one's results.
    if (asked !== query.value) return;
    const sameMode = answer.mode === results.value.mode;
    results.value = answer;
    document.documentElement.style.setProperty("--max-shown", answer.maxShown);
    if (!sameMode || selected.value >= answer.entries.length) selected.value = 0;
}

watch(query, () => {
    clearTimeout(pending);
    const calculating = query.value.includes("calc ");
    pending = setTimeout(refresh, calculating ? 90 : 0);
});

function move(delta) {
    const count = entries.value.length;
    if (!count) return;
    // Wraps, so holding a key walks the whole list and comes back round.
    selected.value = (selected.value + delta + count) % count;
    nextTick(() => list.value?.children[selected.value]?.scrollIntoView({ block: "nearest", inline: "nearest" }));
}

async function activate() {
    if (!current.value) return;
    // A command can ask to lead somewhere rather than close — an
    // `autocomplete` action, or a calculation with nothing to copy yet.
    const next = await invoke("activate", { query: query.value, id: current.value.id });
    if (next) {
        query.value = next;
        focusField();
    }
}

function openInCalculator() {
    invoke("open_in_calculator", { expression: current.value?.comment ?? "" });
}

function onKeydown(event) {
    // Vertical in a list, horizontal in the wallpaper strip.
    const forward = isWallpapers.value ? "ArrowRight" : "ArrowDown";
    const back = isWallpapers.value ? "ArrowLeft" : "ArrowUp";

    switch (event.key) {
        case "Escape":
            invoke("dismiss");
            break;
        case forward:
            move(1);
            break;
        case back:
            move(-1);
            break;
        case "Enter":
            if (event.ctrlKey && isCalc.value) openInCalculator();
            else activate();
            break;
        case "Tab":
            move(event.shiftKey ? -1 : 1);
            break;
        // The vim keys the shell's launcher answers to, so muscle memory
        // carries over.
        case "j":
        case "n":
            if (!event.ctrlKey) return;
            move(1);
            break;
        case "k":
        case "p":
            if (!event.ctrlKey) return;
            move(-1);
            break;
        default:
            return;
    }
    event.preventDefault();
}

function focusField() {
    nextTick(() => field.value?.focus());
}

// The window is resized to whatever the pane needs, so it never shows a
// sheet of glass with a hole in it. Measured rather than calculated: the row
// count is not the only thing that changes the height.
let lastSize = "";
function syncWindowSize() {
    const node = pane.value;
    if (!node) return;
    const height = Math.ceil(node.getBoundingClientRect().height);
    const width = isWallpapers.value ? 1040 : 720;
    const size = `${width}x${height}`;
    if (size === lastSize || height < 40) return;
    lastSize = size;
    invoke("resize", { width, height });
}

onMounted(async () => {
    window.addEventListener("keydown", onKeydown);
    // Fires for anything that changes the pane's height: a mode switch, a
    // result count, an expression growing a line.
    new ResizeObserver(syncWindowSize).observe(pane.value);
    await refresh();
    focusField();

    // Reopening must not show the last thing that was typed.
    // The payload is the query to open with — empty for a plain open, or a
    // mode prefix when a keybind asked for one.
    await listen("launcher-opened", event => {
        query.value = event.payload ?? "";
        selected.value = 0;
        refresh();
        focusField();
    });
});

onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
    <div ref="pane" class="glass" :class="{ wide: isWallpapers }">
        <div class="search">
            <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="11" cy="11" r="7" />
                <path d="M20 20l-4-4" stroke-linecap="round" />
            </svg>
            <input
                ref="field"
                v-model="query"
                type="text"
                placeholder="Search for apps and commands…"
                spellcheck="false"
                autocomplete="off"
            />
        </div>

        <div class="rule"></div>

        <template v-if="entries.length">
            <div class="section">{{ results.label }}</div>

            <div v-if="isWallpapers" ref="list" class="wallpapers">
                <WallpaperTile
                    v-for="(entry, index) in entries"
                    :key="entry.id"
                    :entry="entry"
                    :current="index === selected"
                    @mouseenter="selected = index"
                    @click="activate"
                />
            </div>

            <div v-else ref="list" class="results">
                <ResultRow
                    v-for="(entry, index) in entries"
                    :key="entry.id || entry.name"
                    :entry="entry"
                    :current="index === selected"
                    @mouseenter="selected = index"
                    @click="activate"
                />
            </div>
        </template>

        <div v-else class="empty">
            <div class="headline">No results</div>
            <div class="hint">Try a different search</div>
        </div>

        <div class="rule"></div>

        <div class="footer">
            <span v-if="isCalc && current?.id">
                <kbd>Ctrl</kbd> <kbd>↵</kbd> to open in a calculator
            </span>
            <span v-else>{{ entries.length === 1 ? "1 result" : `${entries.length} results` }}</span>
            <span class="action">
                {{ results.action }}
                <kbd>↵</kbd>
            </span>
        </div>
    </div>
</template>

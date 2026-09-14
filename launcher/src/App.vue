<script setup>
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";

import ResultRow from "./components/ResultRow.vue";

const query = ref("");
const entries = ref([]);
const selected = ref(0);
const field = ref(null);
const list = ref(null);

// Every keystroke asks the backend to rank the whole list again. That is a
// few hundred microseconds for a thousand apps, so there is no debounce: a
// launcher that lags behind the keyboard is the one thing it may not do.
async function refresh() {
    const asked = query.value;
    const results = await invoke("search", { query: asked });
    // A slower earlier request must not overwrite a newer one's results.
    if (asked !== query.value) return;
    entries.value = results;
    selected.value = 0;
}

watch(query, refresh);

const current = computed(() => entries.value[selected.value]);

function move(delta) {
    if (!entries.value.length) return;
    const count = entries.value.length;
    // Wraps, so holding Down walks the whole list and comes back round.
    selected.value = (selected.value + delta + count) % count;
    scrollSelectedIntoView();
}

function scrollSelectedIntoView() {
    nextTick(() => {
        const row = list.value?.children[selected.value];
        row?.scrollIntoView({ block: "nearest" });
    });
}

async function activate() {
    if (!current.value) return;
    await invoke("launch", { id: current.value.id });
}

function onKeydown(event) {
    switch (event.key) {
        case "Escape":
            invoke("dismiss");
            break;
        case "ArrowDown":
            move(1);
            break;
        case "ArrowUp":
            move(-1);
            break;
        case "Enter":
            activate();
            break;
        case "Tab":
            move(event.shiftKey ? -1 : 1);
            break;
        // The vim keys the shell's launcher answers to, so the muscle memory
        // carries over.
        case "j":
        case "n":
            if (event.ctrlKey) move(1);
            else return;
            break;
        case "k":
        case "p":
            if (event.ctrlKey) move(-1);
            else return;
            break;
        default:
            return;
    }
    event.preventDefault();
}

function focusField() {
    nextTick(() => field.value?.focus());
}

onMounted(async () => {
    window.addEventListener("keydown", onKeydown);
    await refresh();
    focusField();

    // Reopening must not show the last thing that was typed.
    await listen("launcher-opened", () => {
        query.value = "";
        selected.value = 0;
        refresh();
        focusField();
    });
});

onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
    <div class="glass">
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
            <div class="section">Applications</div>
            <div ref="list" class="results">
                <ResultRow
                    v-for="(entry, index) in entries"
                    :key="entry.id"
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
            <span>{{ entries.length === 1 ? "1 result" : `${entries.length} results` }}</span>
            <span class="action">
                Open
                <kbd>↵</kbd>
            </span>
        </div>
    </div>
</template>

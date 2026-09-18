<script setup>
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from "vue";

import ResultRow from "./components/ResultRow.vue";
import WallpaperTile from "./components/WallpaperTile.vue";

const query = ref("");
// shallowRef, not ref: a result set is replaced wholesale and never mutated,
// so making every entry object deeply reactive on each keystroke is work with
// nothing to show for it.
const results = shallowRef({ mode: "apps", label: "Applications", action: "Open", maxShown: 8, entries: [] });
const selected = ref(0);
const field = ref(null);
const list = ref(null);

// The selection is one travelling pane rather than a class on whichever row
// happens to be current: a highlight that moves reads as one object sliding,
// where a background swapping between rows reads as two rows blinking.
const cursor = ref({ top: 0, height: 0, shown: false, travelling: false });

const pane = ref(null);
const content = ref(null);
const entries = computed(() => results.value.entries);
const current = computed(() => entries.value[selected.value]);
const isWallpapers = computed(() => results.value.mode === "wallpapers");
const isCalc = computed(() => results.value.mode === "calc");
const completion = computed(() => (isCalc.value ? "" : current.value?.completion || current.value?.name || ""));

// Every keystroke re-ranks in the backend. No debounce: ranking is a few
// hundred microseconds and a launcher that lags the keyboard is the one thing
// it may not do. The calculator is the exception — it shells out to qalc, so
// it waits for a pause rather than spawning a process per character.
let pending = null;
// The query the list on screen answers, which is not always the one in the
// field: the search for the last keystroke may still be on its way back.
let answered = "";
async function refresh() {
    const asked = query.value;
    const answer = await invoke("search", { query: asked });
    // A slower earlier request must not overwrite a newer one's results.
    if (asked !== query.value) return;
    results.value = answer;
    answered = asked;
    document.documentElement.style.setProperty("--max-shown", answer.maxShown);
    // A new list is read from the top. The old position means nothing in it:
    // every keystroke re-ranks, so row three is simply a different thing now,
    // and the best match is the first one.
    selected.value = 0;
}

watch(query, () => {
    clearTimeout(pending);
    const calculating = query.value.includes("calc ");
    pending = setTimeout(refresh, calculating ? 90 : 0);
});

// Rows on their way out stay in the DOM until they have faded, ahead of the
// ones replacing them, so counting them finds the wrong row for an index.
function rowAt(index) {
    return list.value?.querySelectorAll(":is(.row, .tile):not(.row-leave-active)")[index] ?? null;
}

// Placed against the current row's real box rather than a row count, so it is
// right for the wallpaper strip and for any row that is not 44px tall.
function syncCursor(travelling) {
    const row = rowAt(selected.value);
    if (!row || isWallpapers.value) {
        cursor.value = { ...cursor.value, shown: false };
        return;
    }
    cursor.value = { top: row.offsetTop, height: row.offsetHeight, shown: true, travelling };
}

function move(delta) {
    const count = entries.value.length;
    if (!count) return;
    // Wraps, so holding a key walks the whole list and comes back round.
    selected.value = (selected.value + delta + count) % count;
}

// Walking the list slides the highlight and scrolls smoothly; a new set of
// results places the highlight without a journey across rows that are
// themselves still moving, and starts the list from its top again — it keeps
// its scroll position otherwise, with the best match somewhere above it.
watch([entries, selected], ([arrived], [shown]) => {
    const replaced = arrived !== shown;
    nextTick(() => {
        syncCursor(!replaced);
        if (replaced) list.value?.scrollTo({ top: 0, left: 0, behavior: "instant" });
        else rowAt(selected.value)?.scrollIntoView({ block: "nearest", inline: "nearest" });
    });
});

// A row comes to be under the pointer two ways: the pointer moved onto it, or
// the list moved beneath a pointer that is standing still — every keystroke
// reflows it, and the window itself opens under wherever the mouse was left.
// WebKit reports the two identically. Selecting on both let a parked mouse
// take the selection off the top match, and then scroll the list to follow
// itself, which brought the next row under it and did it again. So a row
// notes where the pointer was when it arrived, and hovering counts only once
// that has changed.
let pointerAt = null;

function notePointer(event) {
    pointerAt = { x: event.clientX, y: event.clientY };
}

function hover(event, index) {
    const moved = pointerAt && (event.clientX !== pointerAt.x || event.clientY !== pointerAt.y);
    notePointer(event);
    if (moved) selected.value = index;
}

// A click names its own row: hovering no longer has to have selected it first.
function pick(index) {
    selected.value = index;
    activate();
}

async function activate() {
    // Enter can outrun the search it follows. Typed quickly, it arrives while
    // the list is still the answer to an earlier keystroke, and the top of
    // that list is not what was asked for.
    if (answered !== query.value) {
        clearTimeout(pending);
        await refresh();
    }
    if (!current.value) return;
    // A command can ask to lead somewhere rather than close — an
    // `autocomplete` action, or a calculation with nothing to copy yet.
    const next = await invoke("activate", { query: query.value, id: current.value.id });
    if (next) {
        query.value = next;
        focusField();
    }
}

// Tab fills the field with the selected result rather than running it: the
// way to say "this one, but let me keep typing". An action row completes to
// its own prefix, so tabbing `>sch` lands on `>scheme ` ready for a name.
function autocomplete() {
    const entry = current.value;
    if (!entry || isCalc.value) return;
    const completion = entry.completion || entry.name;
    if (query.value === completion) return;
    query.value = completion;
    focusField();
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
            autocomplete();
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

// The pane's height is animated rather than the window's.
//
// The window used to be resized on every keystroke, which is what made the
// whole thing jump: a layer surface resize is a compositor round trip, and
// the webview relayouts against the new size a frame later. The surface is
// a fixed sheet now — transparent everywhere the pane is not — and only the
// pane grows, as a plain CSS height transition.
let frame = 0;

// The first sizing after the launcher appears is not animated.
//
// The pane keeps the height it had when it was last closed, so reopening it
// with a different number of results animated it from the old height to the
// new one — right as it faded in, which read as a twitch rather than as a
// transition. Opening is not a resize; it is an arrival.
let settleInstantly = true;

function syncPaneHeight() {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
        const glass = pane.value;
        const inner = content.value;
        if (!glass || !inner) return;

        if (settleInstantly) {
            settleInstantly = false;
            glass.classList.add("instant");
            glass.style.height = `${Math.ceil(inner.getBoundingClientRect().height)}px`;
            // Read back the layout before letting the transition return, or
            // the class comes off in the same frame and animates anyway.
            void glass.offsetHeight;
            glass.classList.remove("instant");
            return;
        }
        glass.style.height = `${Math.ceil(inner.getBoundingClientRect().height)}px`;
    });
}

// Anywhere outside the pane closes it. The surface covers the screen so that
// the pane can change size without the window doing so, which means the
// empty space around it is ours to answer for.
function onBackdrop(event) {
    if (event.target === event.currentTarget) invoke("dismiss");
}

onMounted(async () => {
    window.addEventListener("keydown", onKeydown);
    // Fires for anything that changes the content's height: a mode switch, a
    // result count, an expression growing a line.
    new ResizeObserver(syncPaneHeight).observe(content.value);
    await refresh();
    focusField();
    nextTick(() => syncCursor(false));

    // Reset on the way out, not on the way in: by the time the window is
    // shown again the list is already the right one, so the first painted
    // frame is correct and nothing has to be waited for.
    await listen("launcher-closed", () => {
        query.value = "";
        settleInstantly = true;
        // Wherever the pointer is next time is where it was left, not a move.
        pointerAt = null;
        refresh();
    });

    await listen("launcher-shown", () => focusField());

    // Only sent when a keybind asked for a particular mode — `>wallpaper `
    // for the picker, `>calc ` for the calculator.
    await listen("launcher-opened", event => {
        query.value = event.payload ?? "";
        settleInstantly = true;
        refresh();
        focusField();
    });
});

onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
    <div class="stage" @mousedown="onBackdrop">
        <div ref="pane" class="glass" :class="{ wide: isWallpapers }">
            <div ref="content" class="content">
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
                    <!-- What Tab would do, named rather than guessed at. -->
                    <span v-if="completion && completion !== query" class="hint">
                        Autocomplete
                        <kbd>Tab</kbd>
                    </span>
                </div>

                <div class="rule"></div>

                <template v-if="entries.length">
                    <div class="section">{{ results.label }}</div>

                    <TransitionGroup
                        v-if="isWallpapers"
                        :ref="el => (list = el?.$el ?? el)"
                        tag="div"
                        name="row"
                        class="wallpapers"
                    >
                        <WallpaperTile
                            v-for="(entry, index) in entries"
                            :key="entry.id"
                            :entry="entry"
                            :current="index === selected"
                            @mouseenter="notePointer"
                            @mousemove="hover($event, index)"
                            @click="pick(index)"
                        />
                    </TransitionGroup>

                    <div v-else ref="list" class="results">
                        <!-- The selection, as one pane that travels. Behind the
                             rows, so their text is never dimmed by it. -->
                        <div
                            class="cursor"
                            :class="{ shown: cursor.shown, travelling: cursor.travelling }"
                            :style="{ transform: `translateY(${cursor.top}px)`, height: `${cursor.height}px` }"
                        ></div>

                        <TransitionGroup tag="div" name="row" class="rows">
                            <!-- No `current` prop: the selection is the
                                 travelling highlight, so moving it must not
                                 patch every row in the list. -->
                            <ResultRow
                                v-for="(entry, index) in entries"
                                :key="entry.id || entry.name"
                                :entry="entry"
                                @mouseenter="notePointer"
                                @mousemove="hover($event, index)"
                                @click="pick(index)"
                            />
                        </TransitionGroup>
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
        </div>
    </div>
</template>

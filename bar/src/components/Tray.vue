<script setup>
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { inject, onUnmounted, ref } from "vue";

import { whenPointerLeaves } from "../leaving.js";
import { closed, opens } from "../overhang.js";

defineProps({
    items: { type: Array, default: () => [] }
});

const popout = inject("popout");

// The open menu, and which item it belongs to. One at a time: a tray with
// two menus open is two applications both thinking they have the pointer.
const menu = ref({ key: "", x: 0, entries: [] });

// Icons the webview could not load. A blank box says nothing; the placeholder
// glyph at least says "there is an item here".
const broken = ref({});

// Icons are absolute paths the backend resolved, or data URIs it built from
// the raw pixels an item handed over. Only the former needs the asset
// protocol; a data URI is already loadable.
function source(icon) {
    return icon.startsWith("data:") ? icon : convertFileSrc(icon);
}

// Left click is whatever the application decided it is — often "show the
// window", sometimes nothing at all. The coordinates are the icon's, because
// an application may put its own menu there.
function activate(item, event) {
    const box = event.currentTarget.getBoundingClientRect();
    invoke("tray_activate", { key: item.key, x: Math.round(box.left), y: Math.round(box.bottom) });
    close();
}

function middle(item, event) {
    const box = event.currentTarget.getBoundingClientRect();
    invoke("tray_secondary", { key: item.key, x: Math.round(box.left), y: Math.round(box.bottom) });
}

async function openMenu(item, event) {
    if (menu.value.key === item.key) return close();
    const box = event.currentTarget.getBoundingClientRect();
    const entries = await invoke("tray_menu", { key: item.key });
    menu.value = { key: item.key, x: box.left + box.width / 2, entries };
    place();
}

// Hovering opens it too, the way the shell's bar does: a tray icon whose menu
// is the whole point of it should not need to be right-clicked to admit that.
// Just long enough that sweeping the pointer across the row does not open
// four menus on the way past, and no longer — a menu that takes a beat to
// appear feels like the bar is thinking about it.
const HOLD = 70;

let hovering = null;
let icons = {};

function onEnter(item, event) {
    clearTimeout(hovering);
    const target = event.currentTarget;
    icons[item.key] = target;
    hovering = setTimeout(() => openMenu(item, { currentTarget: target }), HOLD);
}

function onLeave() {
    clearTimeout(hovering);
}

function close() {
    stopWatching?.();
    stopWatching = null;
    menu.value = { key: "", x: menu.value.x, entries: [] };
    popout.close();
}

// The menu is its own popout, so it needs its own answer to "has the pointer
// gone" — the bar's sweep only knows about the panels it opened itself.
let stopWatching = null;

function watchForLeaving() {
    stopWatching?.();
    stopWatching = whenPointerLeaves(
        () => [
            panel.value?.getBoundingClientRect(),
            icons[menu.value.key]?.getBoundingClientRect()
        ],
        close
    );
}

onUnmounted(() => stopWatching?.());

async function choose(entry) {
    if (!entry.enabled || entry.kind === "separator" || entry.children.length) return;
    await invoke("tray_click", { key: menu.value.key, id: entry.id });
    close();
}

const panel = ref(null);

// The menu is a popout like any other, so the surface has to be told to let
// the pointer reach it — see `reach` in lib.rs.
function place() {
    requestAnimationFrame(() => {
        const box = panel.value?.getBoundingClientRect();
        if (box) popout.reach(box);
        watchForLeaving();
    });
}
</script>

<template>
    <div v-if="items.length" class="section tray">
        <TransitionGroup name="tray">
            <div
                v-for="item in items"
                :key="item.key"
                class="pill button icon-only tray-item"
                :class="{ attention: item.status === 'NeedsAttention', open: menu.key === item.key }"
                @click="activate(item, $event)"
                @click.middle="middle(item, $event)"
                @contextmenu.prevent="openMenu(item, $event)"
                @mouseenter="onEnter(item, $event)"
                @mouseleave="onLeave"
            >
                <img
                    v-if="item.icon && !broken[item.key]"
                    class="icon"
                    :src="source(item.icon)"
                    alt=""
                    draggable="false"
                    @error="broken[item.key] = true"
                />
                <span v-else class="glyph material-symbols-rounded">web_asset</span>
            </div>
        </TransitionGroup>
    </div>

    <!-- The item's own menu, drawn here rather than by the application: an
         X11 menu window has nowhere to go on a layer surface. -->
    <Transition name="popout" @before-enter="opens" @after-leave="closed">
        <div
            v-if="menu.entries.length"
            ref="panel"
            class="popout menu"
            :style="{ transform: `translateX(${Math.max(8, menu.x - 110)}px)` }"
            @mouseleave="close"
        >
            <template v-for="entry in menu.entries" :key="entry.id">
                <div v-if="entry.kind === 'separator'" class="divider"></div>
                <div
                    v-else
                    class="entry"
                    :class="{ disabled: !entry.enabled, current: entry.checked }"
                    @click="choose(entry)"
                >
                    <span v-if="entry.kind === 'checkmark'" class="glyph material-symbols-rounded">
                        {{ entry.checked ? "check_box" : "check_box_outline_blank" }}
                    </span>
                    <span class="label">{{ entry.label }}</span>
                    <span v-if="entry.children.length" class="trailing">›</span>
                </div>

                <!-- Submenus are already in hand, so they are shown inline
                     rather than opening a second panel to the side that has
                     nowhere to go on a narrow screen. -->
                <div
                    v-for="child in entry.children"
                    :key="`${entry.id}-${child.id}`"
                    class="entry child"
                    :class="{ disabled: !child.enabled, current: child.checked }"
                    @click="choose(child)"
                >
                    <span class="label">{{ child.label }}</span>
                </div>
            </template>
        </div>
    </Transition>
</template>

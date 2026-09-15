<script setup>
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, onMounted, provide, ref, shallowRef } from "vue";

import ActiveWindow from "./components/ActiveWindow.vue";
import Clock from "./components/Clock.vue";
import FeaturesButton from "./components/FeaturesButton.vue";
import FirewallButton from "./components/FirewallButton.vue";
import Media from "./components/Media.vue";
import OsIcon from "./components/OsIcon.vue";
import Popout from "./components/Popout.vue";
import PowerButton from "./components/PowerButton.vue";
import SpecialWorkspaces from "./components/SpecialWorkspaces.vue";
import StatusIcons from "./components/StatusIcons.vue";
import SystemStats from "./components/SystemStats.vue";
import Tray from "./components/Tray.vue";
import Visualiser from "./components/Visualiser.vue";
import Workspaces from "./components/Workspaces.vue";

// Three feeds, and the front end polls none of them: Hyprland pushes when
// something moves, the kernel sampler pushes each second when a reading
// changed, and the slow one pushes every few seconds. The webview is idle in
// between, which is the whole point.
const hypr = ref({
    workspaces: [],
    specials: [],
    active: { title: "", class: "" },
    keyboard: { layout: "", capsLock: false, numLock: false }
});

const system = shallowRef({
    cpu: 0,
    memory: 0,
    memoryUsedGb: 0,
    memoryTotalGb: 0,
    battery: null,
    volume: null,
    microphone: null,
    network: { kind: "", strength: 0 }
});

const services = shallowRef({
    power: { profile: "", available: [] },
    guards: { connected: false, pending: 0, rules: 0 },
    features: [],
    bluetooth: { powered: false, connected: 0 }
});

const tray = shallowRef([]);
const media = shallowRef(null);
// The visualiser's frames, which arrive far more often than anything else on
// the bar and are the one feed that is dropped rather than queued.
const spectrum = shallowRef({ bars: [], live: false });

// The bar's own options, from shell.json. Read once: they change when a
// person edits their config, not on a tick.
// Which screen this particular bar is on. Only meaningful with more than one,
// and only used when the config asks for per-monitor workspaces.
const output = ref("");

// What the bar is made of and in what order — the user's config, not an
// order chosen here. A config that moves the clock moves it in this bar too.
const layout = shallowRef({
    entries: [],
    stats: { cpu: true, ram: true, gpu: true },
    status: {
        showNetwork: true,
        showBluetooth: true,
        showBattery: true,
        showAudio: false,
        showMicrophone: false,
        showKbLayout: false,
        showLockStatus: false
    }
});

const options = shallowRef({
    shown: 5,
    occupiedBg: true,
    activeTrail: true,
    showWindows: false,
    label: "",
    occupiedLabel: "",
    activeLabel: ""
});

// With per-monitor workspaces on, each bar shows only the workspaces that
// live on its own screen; otherwise every bar shows the same row.
const workspaces = computed(() => {
    if (!options.value.perMonitor || !output.value) return hypr.value.workspaces;
    return hypr.value.workspaces.filter(workspace => workspace.monitor === output.value);
});

const popout = ref({ id: "", x: 0 });

// Closing is deferred by a beat so that crossing the gap between the pill and
// the panel it opened does not count as leaving.
let closing = null;

// The element that opened the panel, kept so the sweep below can ask whether
// the pointer is still on it. Not reactive: it is bookkeeping, not state the
// template reads.
let opener = null;

function open(id, event) {
    clearTimeout(closing);
    opener = event.currentTarget;
    const box = opener.getBoundingClientRect();
    popout.value = { id, x: box.left + box.width / 2 };
}

function close() {
    clearTimeout(closing);
    closing = setTimeout(() => {
        opener = null;
        popout.value = { id: "", x: popout.value.x };
        // Hand the screen back: while nothing is open, the only part of the
        // surface that may swallow the pointer is the strip itself.
        invoke("reach", {});
    }, 130);
}

// The surface is taller than the bar, and everything below the strip belongs
// to the window underneath — so the open panel's own box is the only extra
// the bar asks to be able to reach, measured once it has actually laid out.
function reachPanel(box) {
    // Stretched up to meet the bar. The strip and the panel are two separate
    // rectangles in the surface's input region, and the gap between them is
    // not part of either — so the pointer physically left the surface on its
    // way from one to the other, which is not something the page can be told
    // about afterwards.
    const bar = document.querySelector(".bar")?.getBoundingClientRect();
    const top = bar ? Math.min(box.y, bar.bottom - 1) : box.y;

    invoke("reach", {
        popout: {
            x: Math.floor(box.x),
            y: Math.floor(top),
            width: Math.ceil(box.width),
            height: Math.ceil(box.bottom - top)
        }
    });
}

function keep() {
    clearTimeout(closing);
}

// Scrolling the bar itself, the way the shell's does: volume over the left
// half, brightness over the right. Anything with its own wheel handler — the
// workspace row, the volume pill — stops the event before it gets here.
function onWheel(event) {
    const step = event.deltaY > 0 ? -5 : 5;
    if (event.clientX < window.innerWidth / 2) invoke("volume", { delta: step });
    else invoke("brightness", { delta: step });
}

// Every item on the bar opens its own popout the same way, so the handlers
// are handed down rather than threaded through as props on each one.
provide("popout", { open, close, keep, reach: reachPanel });

// The pointer leaving the bar is not something this page is told about.
//
// The surface accepts the pointer only inside the strip and whatever panel is
// open, so crossing out of that region onto a window is not a `mouseleave` —
// no event arrives at all. Two things then stick: the panel that was open
// stays open, and the engine's own `:hover` stays on whatever the pointer was
// over, because it never learned otherwise. Moving to something else on the
// bar clears both, which is exactly why it only misbehaves on the way out.
//
// So the compositor is asked where the pointer actually is, and the hover is
// cleared by hand.
let sweeping = null;

function boxOf(selector) {
    return document.querySelector(selector)?.getBoundingClientRect();
}

function contains(box, x, y, slack = 6) {
    return (
        box &&
        x >= box.left - slack &&
        x <= box.right + slack &&
        y >= box.top - slack &&
        y <= box.bottom + slack
    );
}

// What the pointer is over, tracked by hand.
//
// The bar cannot use CSS `:hover` for this. The engine sets it when the
// pointer arrives and clears it when the pointer leaves — but on this surface
// the pointer often leaves without the page being told, so the highlight
// stayed lit on whatever was last under it. A class this code sets and clears
// is a fact this code knows.
const HOVERABLE = ".pill, .stat, .active, .media, .tray-item, .logo";
let hovered = null;

function setHovered(element) {
    if (hovered === element) return;
    hovered?.classList.remove("hovered");
    hovered = element;
    hovered?.classList.add("hovered");
}

function clearStuckHover() {
    setHovered(null);
}

function gone() {
    misses = 0;
    stopSweeping();
    close();
    clearStuckHover();
}

function stopSweeping() {
    clearInterval(sweeping);
    sweeping = null;
}

// Crossing from the bar down onto the panel it opened passes through the gap
// between them, and a single miss there would close the thing being reached
// for. So a miss has to happen twice, and the gap itself counts as inside:
// the bar and the open panel are treated as one region, bounding box and all.
let misses = 0;

function reachableBox() {
    const bar = boxOf(".bar");
    const panel = boxOf(".popout");
    if (!bar) return panel;
    if (!panel) return bar;
    return {
        left: Math.min(bar.left, panel.left),
        right: Math.max(bar.right, panel.right),
        top: Math.min(bar.top, panel.top),
        bottom: Math.max(bar.bottom, panel.bottom)
    };
}

function startSweeping() {
    if (sweeping) return;
    misses = 0;
    sweeping = setInterval(async () => {
        const position = await invoke("pointer");
        // No compositor to ask: the events are all there is.
        if (!position) return;

        const [x, y] = position;
        const here =
            contains(reachableBox(), x, y, 10) || contains(boxOf(".logo"), x, y, 10);

        if (here) {
            misses = 0;
            return;
        }
        if (++misses >= 2) gone();
    }, 120);
}

function watchThePointer() {
    // An event saying the pointer left is a hint, not a verdict.
    //
    // Crossing from the bar down onto a panel produces exactly the same event
    // as leaving the bar altogether — the pointer really does cross a strip
    // that belongs to neither — so acting on it directly closed the panel the
    // pointer was reaching for. These only wake the sweep; where the pointer
    // actually is decides.
    document.addEventListener("mouseout", event => {
        if (!event.relatedTarget) startSweeping();
    });
    document.documentElement.addEventListener("mouseleave", startSweeping);

    // Focus going elsewhere is unambiguous: the launcher opened, or something
    // took the keyboard. Nothing to double-check.
    window.addEventListener("blur", gone);

    // And the sweep runs only while the pointer is thought to be on the bar,
    // so a desktop nobody is touching costs nothing.
    document.addEventListener("mouseover", event => {
        setHovered(event.target.closest?.(HOVERABLE) ?? null);
        startSweeping();
    });
}

onMounted(async () => {
    watchThePointer();

    await listen("hypr", event => (hypr.value = event.payload));
    await listen("system", event => (system.value = event.payload));
    await listen("services", event => (services.value = event.payload));
    await listen("tray", event => (tray.value = event.payload));
    await listen("media", event => (media.value = event.payload));
    await listen("spectrum", event => {
        const [bars, live] = event.payload;
        spectrum.value = { bars, live };
    });
    // The first paint should not wait for something to happen — and the
    // pushed feeds only fire on a change, whose first one lands before these
    // listeners exist.
    hypr.value = await invoke("state");
    options.value = await invoke("bar_config");
    layout.value = await invoke("layout");
    output.value = await invoke("monitor");
    const [items, now, playing] = await invoke("snapshot");
    tray.value = items;
    services.value = now;
    media.value = playing;
});
</script>

<template>
    <!-- Outside the bar, not in it: the pill is masked to the mark's contour,
         and a mask clips its element's children too — so a mark drawn inside
         the bar would be cut away by its own silhouette. -->
    <OsIcon v-if="layout.entries.includes('logo')" />

    <div class="bar" @wheel.prevent="onWheel">
        <!-- The pill itself. It is a layer of its own because it carries the
             mask that cuts it to the mark's contour, and a mask clips
             everything its element paints — including panels its children
             open below the bar. -->
        <div class="bar-face"></div>

        <!-- Behind everything, the width of the screen: the spectrum is the
             surface reacting to sound, not a widget sitting in a slot. -->
        <Visualiser
            v-if="layout.entries.includes('visualiser')"
            :bars="spectrum.bars"
            :live="spectrum.live"
        />

        <!-- Laid out from the config's entry list. Anything the config names
             that this bar has no component for is skipped rather than
             guessed at. -->
        <template v-for="(entry, index) in layout.entries" :key="`${entry}-${index}`">
            <div v-if="entry === 'spacer'" class="spacer"></div>
            <Workspaces
                v-else-if="entry === 'workspaces'"
                :workspaces="workspaces"
                :options="options"
            />
            <SpecialWorkspaces v-else-if="entry === 'specials'" :specials="hypr.specials" />
            <ActiveWindow v-else-if="entry === 'activeWindow'" :active="hypr.active" />
            <Media v-else-if="entry === 'media'" :playing="media" />
            <SystemStats
                v-else-if="entry === 'sysStats'"
                :snapshot="system"
                :metrics="layout.stats"
            />
            <div v-else-if="entry === 'firewall'" class="section guards">
                <FirewallButton :guards="services.guards" />
                <FeaturesButton
                    v-if="!layout.entries.includes('features')"
                    :features="services.features"
                />
            </div>
            <div v-else-if="entry === 'features'" class="section guards">
                <FeaturesButton :features="services.features" />
            </div>
            <Tray v-else-if="entry === 'tray'" :items="tray" />
            <StatusIcons
                v-else-if="entry === 'statusIcons'"
                :snapshot="system"
                :keyboard="hypr.keyboard"
                :bluetooth="services.bluetooth"
                :profile="services.power.profile"
                :status="layout.status"
            />
            <Clock v-else-if="entry === 'clock'" />
            <PowerButton v-else-if="entry === 'power'" />
        </template>
    </div>

    <Popout
        :id="popout.id"
        :x="popout.x"
        :snapshot="system"
        :services="services"
        :playing="media"
        :active="hypr.active"
        @keep="keep"
        @leave="close"
        @placed="reachPanel"
    />
</template>

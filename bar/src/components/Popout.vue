<script setup>
import { invoke } from "@tauri-apps/api/core";
import { computed, ref, watch } from "vue";

import { closed, opens } from "../overhang.js";

const props = defineProps({
    playing: { type: Object, default: null },
    // Which item is being hovered, "" for none.
    id: { type: String, default: "" },
    // Centre of the item that opened it, in page coordinates.
    x: { type: Number, default: 0 },
    snapshot: { type: Object, required: true },
    services: { type: Object, required: true },
    active: { type: Object, default: () => ({ title: "", class: "" }) }
});

const emit = defineEmits(["keep", "leave", "placed"]);

const panel = ref(null);

// One width for every panel, fixed in the stylesheet as well as here.
//
// It used to be measured after each render, which meant every change of
// content — a scan coming back, a description appearing under a switch —
// moved the panel sideways under the pointer. A constant cannot do that.
const WIDTH = 340;

// Kept inside the screen: an item at either end would otherwise open a panel
// with half of it off the edge.
const left = computed(() => {
    const margin = 8;
    return Math.min(Math.max(props.x - WIDTH / 2, margin), window.innerWidth - WIDTH - margin);
});

const network = computed(() => props.snapshot.network);
const volume = computed(() => props.snapshot.volume);
const microphone = computed(() => props.snapshot.microphone);
const battery = computed(() => props.snapshot.battery);
const power = computed(() => props.services.power);
const guards = computed(() => props.services.guards);
const bluetooth = computed(() => props.services.bluetooth);

// Scanning and pairing lists are only worth having while their panel is open,
// and both cost a process, so they are fetched on opening rather than kept.
const networks = ref([]);
const wired = ref([]);
const devices = ref([]);
const sinks = ref([]);
const sources = ref([]);
const scanning = ref(false);
const joining = ref("");
const password = ref("");
const failure = ref("");
const busy = ref(false);

async function loadNetworks() {
    networks.value = await invoke("wifi_list");
    wired.value = await invoke("ethernet_list");
}

// nmcli returns before the scan has finished, so the list is re-read a beat
// later rather than immediately — otherwise the button appears to do nothing.
async function rescan() {
    scanning.value = true;
    await invoke("wifi_rescan");
    setTimeout(async () => {
        await loadNetworks();
        scanning.value = false;
    }, 2200);
}

async function toggleEthernet(device) {
    await invoke("ethernet_set", { interface: device.interface, connect: !device.connected });
    setTimeout(loadNetworks, 1200);
}

async function loadAudio() {
    const [outputs, inputs] = await invoke("audio_nodes");
    sinks.value = outputs;
    sources.value = inputs;
}

async function chooseNode(kind, node) {
    await invoke("audio_default", { kind, name: node.name });
    setTimeout(loadAudio, 400);
}

async function loadDevices() {
    devices.value = await invoke("bluetooth_devices");
}

// A network that has been joined before, or an open one, needs nothing from
// anybody; anything else asks for the password in place.
async function choose(entry) {
    if (entry.active) return;
    if (entry.secured && !entry.known) {
        joining.value = entry.ssid;
        password.value = "";
        failure.value = "";
        return;
    }
    await join(entry.ssid, "");
}

async function join(ssid, secret) {
    busy.value = true;
    failure.value = "";
    try {
        await invoke("wifi_join", { ssid, password: secret });
        joining.value = "";
        await loadNetworks();
    } catch (error) {
        failure.value = String(error);
    } finally {
        busy.value = false;
    }
}

async function toggleRadio() {
    await invoke("wifi_radio", { on: !network.value.kind });
    setTimeout(loadNetworks, 900);
}

async function toggleBluetooth() {
    await invoke("bluetooth_radio", { on: !bluetooth.value.powered });
    setTimeout(loadDevices, 900);
}

async function toggleDevice(device) {
    await invoke("bluetooth_connect", { address: device.address, connect: !device.connected });
    setTimeout(loadDevices, 900);
}

async function forget(device) {
    await invoke("bluetooth_forget", { address: device.address });
    setTimeout(loadDevices, 600);
}

async function toggleDiscovering() {
    await invoke("bluetooth_discover", { on: !bluetooth.value.discovering });
}

const profileNames = {
    "power-saver": "Power saver",
    balanced: "Balanced",
    performance: "Performance"
};

const profileGlyphs = {
    "power-saver": "energy_savings_leaf",
    balanced: "balance",
    performance: "rocket_launch"
};

// The modes, as the shell's own menu describes them: a switch with no
// explanation is a switch nobody touches.
const features = {
    maxPerf: {
        name: "Maximum performance",
        glyph: "bolt",
        detail: "Pins CPU and GPU at their limits, tuned to this machine"
    },
    antiHeat: {
        name: "Anti-Heat",
        glyph: "ac_unit",
        detail: "Runs cooler at full speed: undervolt and early fans, never power caps"
    },
    lidStay: {
        name: "Stay awake on lid close",
        glyph: "laptop",
        detail: "The lid stops suspending the machine; the screen still turns off"
    },
    caffeine: {
        name: "Caffeine",
        glyph: "coffee",
        detail: "Nothing idles, blanks or locks while this is on"
    },
    gameMode: {
        name: "Game mode",
        glyph: "sports_esports",
        detail: "Quietens the desktop and keeps the compositor out of the way"
    },
    bedMode: {
        name: "Bed mode",
        glyph: "bed",
        detail: "Much more sensitive fan curve for restricted airflow, e.g. on a bed. Fans only — your power profile is untouched"
    }
};

function describe(id) {
    return features[id] ?? { name: id, glyph: "tune", detail: "Hover a mode to see what it does" };
}

// Whichever mode the pointer is over, so its description is the one shown.
const focusedFeature = ref("");

// Picking a profile by hand takes it back from the auto-switcher, which is
// what the shell does too: the two cannot both be driving.
async function choosePower(name) {
    if (props.services.power.dynamic) await invoke("dynamic_profile", { on: false });
    await invoke("power_profile", { profile: name });
}

async function toggleFeature(id) {
    await invoke("feature_toggle", { id });
}

// The time the battery has left, said the way a person would.
// The auto-switcher's current pick, in the words the shell uses for it.
const tierName = computed(() => {
    const tier = props.services.power.dynamicTier;
    if (tier === "yield") return "paused — Max performance on";
    if (!tier) return "starting…";
    return profileNames[tier] ?? tier;
});

const remaining = computed(() => {
    const minutes = battery.value?.minutes;
    if (minutes === null || minutes === undefined) return "";
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    const spell = hours > 0 ? `${hours} hr${hours === 1 ? "" : "s"} ${rest} mins` : `${rest} mins`;
    return battery.value.charging ? `Time until charged: ${spell}` : `Time remaining: ${spell}`;
});

// Charging is a state, not a duration: the bar has no business guessing at
// hours remaining from a single capacity reading.
const batteryLabel = computed(() => {
    if (!battery.value) return "";
    return battery.value.charging ? "Charging" : "On battery";
});

// Spelled out in full here, because the bar itself only has room for the
// short form.
const today = computed(() =>
    new Date().toLocaleDateString([], { weekday: "long", day: "numeric", month: "long", year: "numeric" })
);

// ISO week, which is the one a calendar with a week number on it means.
const week = computed(() => {
    const date = new Date();
    const thursday = new Date(date.getFullYear(), date.getMonth(), date.getDate() + 3 - ((date.getDay() + 6) % 7));
    const first = new Date(thursday.getFullYear(), 0, 4);
    return 1 + Math.round(((thursday - first) / 86_400_000 - 3 + ((first.getDay() + 6) % 7)) / 7);
});

const networkLabel = computed(() => {
    if (network.value.kind === "ethernet") return "Wired";
    if (network.value.kind === "wifi") return "Wireless";
    return "No connection";
});

// Measured after it renders, because the width depends on what is in it, and
// again after it has been moved into place — the box the surface has to make
// reachable is the one it ends up occupying, not the one it was born at.
watch(
    () => props.id,
    id => {
        if (!id) return;
        joining.value = "";
        failure.value = "";
        if (id === "network") loadNetworks();
        if (id === "bluetooth") loadDevices();
        if (id === "volume" || id === "microphone") loadAudio();
        place();
    }
);

// The panel changes size as a scan comes back or a password field opens, and
// the reachable region has to follow it or the pointer falls through.
// Only the height can change now that the width is fixed, and the reachable
// region has to follow it or the pointer falls through the bottom of a panel
// that just grew.
watch([networks, wired, devices, sinks, sources, joining, failure, focusedFeature], place, {
    deep: true
});

function place() {
    requestAnimationFrame(() => {
        const box = panel.value?.getBoundingClientRect();
        if (box) emit("placed", box);
    });
}
</script>

<template>
    <Transition name="popout" @before-enter="opens" @after-leave="closed">
        <div
            v-if="id"
            ref="panel"
            class="popout"
            :style="{ transform: `translateX(${left}px)` }"
            @mouseenter="emit('keep')"
            @mouseleave="emit('leave')"
        >
            <template v-if="id === 'network'">
                <div class="row">
                    <div class="headline">{{ networkLabel }}</div>
                    <button class="toggle" :class="{ on: !!network.kind }" @click="toggleRadio">
                        {{ network.kind ? "On" : "Off" }}
                    </button>
                </div>

                <div v-if="network.kind === 'wifi'" class="meter">
                    <div class="fill" :style="{ width: `${network.strength}%` }"></div>
                </div>

                <div class="caption">
                    {{ networks.length }} network{{ networks.length === 1 ? "" : "s" }} available
                </div>

                <div class="list">
                    <TransitionGroup name="entry">
                        <div
                            v-for="entry in networks"
                            :key="entry.ssid"
                            class="entry"
                            :class="{ current: entry.active }"
                            @click="choose(entry)"
                        >
                            <span class="glyph material-symbols-rounded">
                                {{ entry.secured ? "wifi_lock" : "wifi" }}
                            </span>
                            <span class="label">{{ entry.ssid }}</span>
                            <span class="trailing">{{ entry.strength }}%</span>
                        </div>
                    </TransitionGroup>
                    <div v-if="!networks.length" class="detail">Nothing in range</div>
                </div>

                <button class="wide" :disabled="scanning" @click="rescan">
                    <span class="glyph material-symbols-rounded" :class="{ spinning: scanning }">refresh</span>
                    {{ scanning ? "Scanning…" : "Rescan networks" }}
                </button>

                <!-- Wired devices, which the shell's popout lists under the
                     same roof: a dock or a USB adapter is something you
                     connect and disconnect like anything else. -->
                <template v-if="wired.length">
                    <div class="headline">Ethernet</div>
                    <div class="caption">
                        {{ wired.length }} device{{ wired.length === 1 ? "" : "s" }} available
                    </div>
                    <div class="list">
                        <div
                            v-for="device in wired"
                            :key="device.interface"
                            class="entry"
                            :class="{ current: device.connected }"
                            @click="toggleEthernet(device)"
                        >
                            <span class="glyph material-symbols-rounded">lan</span>
                            <span class="label">{{ device.connection || device.interface }}</span>
                            <span class="trailing">{{ device.connected ? "Connected" : "Off" }}</span>
                        </div>
                    </div>
                </template>

                <!-- Asked for in place rather than in a dialog: the panel is
                     already the thing being looked at. -->
                <form v-if="joining" class="ask" @submit.prevent="join(joining, password)">
                    <input
                        v-model="password"
                        class="field"
                        type="password"
                        :placeholder="`Password for ${joining}`"
                        autofocus
                    />
                    <button class="toggle on" type="submit" :disabled="busy">
                        {{ busy ? "…" : "Join" }}
                    </button>
                </form>
                <div v-if="failure" class="detail wrap alert">{{ failure }}</div>
            </template>

            <template v-else-if="id === 'bluetooth'">
                <div class="row">
                    <div class="headline">Bluetooth</div>
                    <button class="toggle" :class="{ on: bluetooth.powered }" @click="toggleBluetooth">
                        {{ bluetooth.powered ? "On" : "Off" }}
                    </button>
                </div>

                <div class="row">
                    <div class="detail">Discovering</div>
                    <button
                        class="toggle"
                        :class="{ on: bluetooth.discovering }"
                        @click="toggleDiscovering"
                    >
                        {{ bluetooth.discovering ? "On" : "Off" }}
                    </button>
                </div>

                <div class="caption">
                    {{ devices.length }} device{{ devices.length === 1 ? "" : "s" }} available<template
                        v-if="bluetooth.connected"
                    >
                        ({{ bluetooth.connected }} connected)</template
                    >
                </div>

                <div class="list">
                    <div
                        v-for="device in devices"
                        :key="device.address"
                        class="entry"
                        :class="{ current: device.connected }"
                        @click="toggleDevice(device)"
                    >
                        <span class="glyph material-symbols-rounded">
                            {{ device.connected ? "bluetooth_connected" : "bluetooth" }}
                        </span>
                        <span class="label">{{ device.name }}</span>
                        <!-- Forgetting is deliberate, so it is its own target
                             rather than something the row does. -->
                        <button class="ghost" title="Forget" @click.stop="forget(device)">
                            <span class="glyph material-symbols-rounded">close</span>
                        </button>
                    </div>
                    <div v-if="!devices.length" class="detail">Nothing paired</div>
                </div>

                <button class="wide" @click="invoke('open_settings')">
                    <span class="glyph material-symbols-rounded">settings</span>
                    Open settings
                </button>
            </template>

            <template v-else-if="id === 'volume' && volume">
                <div class="headline">{{ volume.muted ? "Muted" : `Volume ${volume.level}%` }}</div>
                <input
                    class="slider"
                    type="range"
                    min="0"
                    max="100"
                    :value="volume.level"
                    @input="invoke('volume_to', { level: Number($event.target.value) })"
                />

                <div class="caption">Output device</div>
                <div class="list">
                    <div
                        v-for="node in sinks"
                        :key="node.name"
                        class="entry"
                        :class="{ current: node.default }"
                        @click="chooseNode('sink', node)"
                    >
                        <span class="glyph material-symbols-rounded">speaker</span>
                        <span class="label">{{ node.description }}</span>
                    </div>
                </div>

                <div class="detail">Click the icon to {{ volume.muted ? "unmute" : "mute" }}</div>

                <button class="wide" @click="invoke('open_settings')">
                    <span class="glyph material-symbols-rounded">settings</span>
                    Open settings
                </button>
            </template>

            <template v-else-if="id === 'microphone' && microphone">
                <div class="headline">
                    {{ microphone.muted ? "Microphone muted" : `Microphone ${microphone.level}%` }}
                </div>
                <input
                    class="slider"
                    type="range"
                    min="0"
                    max="100"
                    :value="microphone.level"
                    @input="invoke('mic_to', { level: Number($event.target.value) })"
                />
                <div class="caption">Input device</div>
                <div class="list">
                    <div
                        v-for="node in sources"
                        :key="node.name"
                        class="entry"
                        :class="{ current: node.default }"
                        @click="chooseNode('source', node)"
                    >
                        <span class="glyph material-symbols-rounded">mic</span>
                        <span class="label">{{ node.description }}</span>
                    </div>
                </div>

                <div class="detail">Click the icon to {{ microphone.muted ? "unmute" : "mute" }}</div>
            </template>

            <template v-else-if="id === 'battery'">
                <template v-if="battery">
                    <div class="headline">{{ remaining || `${battery.level}%` }}</div>
                    <div class="meter">
                        <div
                            class="fill"
                            :class="{ charging: battery.charging }"
                            :style="{ width: `${battery.level}%` }"
                        ></div>
                    </div>
                    <div class="detail">{{ battery.level }}% · {{ batteryLabel }}</div>
                </template>

                <!-- Thermal throttling and the like: the machine is not
                     giving what the profile asks for, and saying so is the
                     difference between a slow laptop and a broken one. -->
                <div v-if="power.degraded" class="warning">
                    <span class="glyph material-symbols-rounded">warning</span>
                    <span>Performance degraded — {{ power.degraded.replaceAll("-", " ") }}</span>
                </div>

                <!-- The profile switch lives here whether or not there is a
                     battery: on a desktop it is the whole popout. One row of
                     dials rather than a list, because they are one choice. -->
                <div v-if="power.available.length" class="dials">
                    <button
                        v-for="name in power.available"
                        :key="name"
                        class="dial"
                        :class="{ on: name === power.profile && !power.dynamic }"
                        :title="profileNames[name] ?? name"
                        @click="choosePower(name)"
                    >
                        <span class="glyph material-symbols-rounded">{{ profileGlyphs[name] ?? "balance" }}</span>
                    </button>

                    <!-- The auto-switcher, which takes the choice over rather
                         than being one of the choices: it is on top of a
                         profile, not instead of one. -->
                    <button
                        class="dial"
                        :class="{ on: power.dynamic }"
                        title="Auto-switch by load"
                        @click="invoke('dynamic_profile', { on: !power.dynamic })"
                    >
                        <span class="glyph material-symbols-rounded">auto_mode</span>
                    </button>
                </div>

                <div v-if="power.dynamic" class="detail">
                    Auto-switching by load{{ power.dynamicTier ? ` — now: ${tierName}` : "" }}
                </div>

                <!-- The modes that change how the machine behaves on battery,
                     each with the sentence that says what it actually does. -->
                <div class="switches">
                    <div
                        v-if="services.bedMode !== null"
                        class="switch-row"
                        @mouseenter="focusedFeature = 'bedMode'"
                        @click="invoke('bed_mode')"
                    >
                        <span class="glyph material-symbols-rounded">bed</span>
                        <span class="label">Bed mode</span>
                        <span class="switch" :class="{ on: services.bedMode }">
                            <span class="knob">
                                <span class="material-symbols-rounded">
                                    {{ services.bedMode ? "check" : "close" }}
                                </span>
                            </span>
                        </span>
                    </div>

                    <div
                        v-for="feature in services.features"
                        :key="feature.id"
                        class="switch-row"
                        @mouseenter="focusedFeature = feature.id"
                        @click="toggleFeature(feature.id)"
                    >
                        <span class="glyph material-symbols-rounded">{{ describe(feature.id).glyph }}</span>
                        <span class="label">{{ describe(feature.id).name }}</span>
                        <span class="switch" :class="{ on: feature.enabled }">
                            <span class="knob">
                                <span class="material-symbols-rounded">{{ feature.enabled ? "check" : "close" }}</span>
                            </span>
                        </span>
                    </div>
                    <!-- Reserved whether or not anything is hovered, so that
                         moving between switches does not resize the panel
                         under the pointer. -->
                    <div class="detail wrap description">{{ describe(focusedFeature).detail }}</div>
                </div>
            </template>

            <template v-else-if="id === 'guards'">
                <div class="headline">{{ guards.connected ? "Protected" : "Guards offline" }}</div>
                <div class="detail">
                    <template v-if="!guards.connected">Neither daemon is answering</template>
                    <template v-else-if="guards.pending > 0">
                        {{ guards.pending }} waiting on you
                    </template>
                    <template v-else>{{ guards.rules }} rules, nothing waiting</template>
                </div>
                <div class="detail">Click to open the security centre</div>
            </template>

            <template v-else-if="id === 'features'">
                <div class="headline">Feature modes</div>
                <div class="list">
                    <div
                        v-for="feature in services.features"
                        :key="feature.id"
                        class="entry"
                        :class="{ current: feature.enabled }"
                    >
                        <span class="label">{{ featureNames[feature.id] ?? feature.id }}</span>
                        <span class="trailing">{{ feature.enabled ? "On" : "Off" }}</span>
                    </div>
                </div>
                <div class="detail">Click to open the menu</div>
            </template>

            <template v-else-if="id === 'brightness'">
                <div class="headline">Brightness {{ snapshot.brightness }}%</div>
                <input
                    class="slider"
                    type="range"
                    min="1"
                    max="100"
                    :value="snapshot.brightness"
                    @input="invoke('brightness_to', { level: Number($event.target.value) })"
                />
                <div class="detail">Scroll the icon to step it</div>
            </template>

            <template v-else-if="id === 'gpu'">
                <div class="headline">Graphics</div>
                <div class="meter"><div class="fill" :style="{ width: `${snapshot.gpu ?? 0}%` }"></div></div>
                <div class="detail">{{ Math.round(snapshot.gpu ?? 0) }}% in use</div>
            </template>

            <template v-else-if="id === 'media' && playing">
                <div class="headline">{{ playing.title || playing.identity }}</div>
                <div v-if="playing.artist" class="detail wrap">{{ playing.artist }}</div>
                <div class="transport">
                    <button
                        class="control"
                        :disabled="!playing.canGoPrevious"
                        @click="invoke('media_control', { action: 'Previous' })"
                    >
                        <span class="glyph material-symbols-rounded">skip_previous</span>
                    </button>
                    <button class="control primary" @click="invoke('media_control', { action: 'PlayPause' })">
                        <span class="glyph material-symbols-rounded">
                            {{ playing.playing ? "pause" : "play_arrow" }}
                        </span>
                    </button>
                    <button
                        class="control"
                        :disabled="!playing.canGoNext"
                        @click="invoke('media_control', { action: 'Next' })"
                    >
                        <span class="glyph material-symbols-rounded">skip_next</span>
                    </button>
                </div>
                <div class="detail">{{ playing.identity }}</div>
            </template>

            <template v-else-if="id === 'cpu'">
                <div class="headline">Processor</div>
                <div class="meter"><div class="fill" :style="{ width: `${snapshot.cpu}%` }"></div></div>
                <div class="detail">{{ Math.round(snapshot.cpu) }}% in use</div>
            </template>

            <template v-else-if="id === 'memory'">
                <div class="headline">Memory</div>
                <div class="meter"><div class="fill" :style="{ width: `${snapshot.memory}%` }"></div></div>
                <div class="detail">
                    {{ snapshot.memoryUsedGb.toFixed(1) }} of {{ snapshot.memoryTotalGb.toFixed(1) }} GiB
                </div>
            </template>

            <template v-else-if="id === 'active'">
                <div class="headline">{{ active.class || "Desktop" }}</div>
                <div class="detail wrap">{{ active.title || "Nothing focused" }}</div>
            </template>

            <template v-else-if="id === 'clock'">
                <div class="headline">{{ today }}</div>
                <div class="detail">Week {{ week }}</div>
            </template>

            <template v-else-if="id === 'logo'">
                <div class="headline">Caelestia++</div>
                <div class="detail">Click to open the launcher</div>
            </template>
        </div>
    </Transition>
</template>

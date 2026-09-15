<script setup>
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, onMounted, onUnmounted, ref, shallowRef } from "vue";

// The security centre and the feature menu, in one window with two tabs —
// the shield and the wrench both land here, on the tab they belong to.
const tab = ref("firewall");

const guards = shallowRef([]);
const features = shallowRef([]);
const startup = shallowRef([]);

// Adding an entry by hand, which is the one thing this panel writes rather
// than toggles.
const newName = ref("");
const newExec = ref("");

const firewall = computed(() => guards.value.find(guard => guard.name === "firewall"));
const protection = computed(() => guards.value.find(guard => guard.name === "protection"));
const current = computed(() => (tab.value === "protection" ? protection.value : firewall.value));

// The modes, described the way the shell's own menu describes them: a switch
// with no explanation is a switch nobody touches.
const descriptions = {
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
};

function describe(id) {
    return descriptions[id] ?? { name: id, glyph: "tune", detail: "" };
}

// The feature hub's own modes, and only those. Bed mode is a fan curve, not
// a feature mode — the shell keeps its switch in the battery popout and so
// does this bar.
const modes = computed(() => features.value.map(feature => ({ ...feature, own: false })));

async function refresh() {
    guards.value = await invoke("guards_detail");
    const [, services] = await invoke("snapshot");
    features.value = services.features;
    if (tab.value === "startup") startup.value = await invoke("startup_list");
}

// The helper writes files and systemctl enables units; both are
// fire-and-forget, so the list is re-read a beat later rather than racing the
// write — the same wait the shell's own service uses.
function rescanLater() {
    setTimeout(async () => (startup.value = await invoke("startup_list")), 400);
}

async function toggleStartup(entry) {
    await invoke("startup_set", { source: entry.source, key: entry.key, enabled: !entry.enabled });
    rescanLater();
}

async function removeStartup(entry) {
    await invoke("startup_remove", { source: entry.source, key: entry.key });
    rescanLater();
}

async function addStartup() {
    if (!newName.value || !newExec.value) return;
    await invoke("startup_add", { name: newName.value, exec: newExec.value });
    newName.value = "";
    newExec.value = "";
    rescanLater();
}

async function toggleMode(mode) {
    await invoke("feature_toggle", { id: mode.id });
    setTimeout(refresh, 250);
}

async function answer(prompt, action, remember) {
    await invoke("guards_verdict", {
        which: tab.value,
        id: prompt.id,
        action,
        remember
    });
    refresh();
}

async function setRule(rule, action) {
    await invoke("guards_set_rule", {
        which: tab.value,
        exe: rule.exe,
        action,
        name: rule.name ?? ""
    });
    setTimeout(refresh, 200);
}

async function forget(rule) {
    await invoke("guards_delete_rule", { which: tab.value, exe: rule.exe });
    setTimeout(refresh, 200);
}

async function toggleEnforcing() {
    await invoke("guards_set_enabled", { which: tab.value, enabled: !current.value?.enabled });
    setTimeout(refresh, 200);
}

// Rules are a long list on a machine that has been used for a while, so the
// panel filters rather than making anybody scroll it.
const filter = ref("");
const rules = computed(() => {
    const needle = filter.value.trim().toLowerCase();
    const all = current.value?.rules ?? [];
    if (!needle) return all;
    return all.filter(rule =>
        `${rule.name ?? ""} ${rule.exe ?? ""}`.toLowerCase().includes(needle)
    );
});

// The basename, which is what a person recognises; the full path is there for
// when two of them share one.
function shortName(rule) {
    return rule.name || (rule.exe ?? "").split("/").pop() || "Unknown";
}

function onKeydown(event) {
    if (event.key === "Escape") invoke("close_panel");
}

let stop = null;

onMounted(async () => {
    window.addEventListener("keydown", onKeydown);
    await refresh();
    // Opened again while already open: the shield and the wrench each say
    // which tab they meant.
    stop = await listen("panel-tab", event => {
        tab.value = event.payload;
        refresh();
    });
    const initial = await invoke("panel_tab");
    if (initial) tab.value = initial;
});

onUnmounted(() => {
    window.removeEventListener("keydown", onKeydown);
    if (stop) stop();
});
</script>

<template>
    <div class="panel-stage" @mousedown.self="invoke('close_panel')">
        <div class="panel">
            <header>
                <nav class="tabs">
                    <button
                        v-for="entry in [
                            { id: 'firewall', name: 'Firewall', glyph: 'gpp_good' },
                            { id: 'protection', name: 'Protection', glyph: 'security' },
                            { id: 'features', name: 'Features', glyph: 'build' },
                            { id: 'startup', name: 'Startup', glyph: 'rocket_launch' }
                        ]"
                        :key="entry.id"
                        class="tab"
                        :class="{ on: tab === entry.id }"
                        @click="((tab = entry.id), refresh())"
                    >
                        <span class="glyph material-symbols-rounded">{{ entry.glyph }}</span>
                        {{ entry.name }}
                    </button>
                </nav>

                <button class="ghost close" title="Close" @click="invoke('close_panel')">
                    <span class="glyph material-symbols-rounded">close</span>
                </button>
            </header>

            <!-- ---- the two guards, which share a protocol and a layout ---- -->
            <section v-if="tab === 'firewall' || tab === 'protection'" class="body">
                <div v-if="!current?.connected" class="warning">
                    <span class="glyph material-symbols-rounded">warning</span>
                    <span>
                        The {{ tab }} daemon is not answering. Rules are kept, but nothing is
                        being enforced.
                    </span>
                </div>

                <template v-else>
                    <div class="row">
                        <div>
                            <div class="headline">{{ current.enabled ? "Enforcing" : "Paused" }}</div>
                            <div class="detail">
                                {{ current.rules.length }} rule{{ current.rules.length === 1 ? "" : "s" }}
                                remembered
                            </div>
                        </div>
                        <span class="switch" :class="{ on: current.enabled }" @click="toggleEnforcing">
                            <span class="knob">
                                <span class="material-symbols-rounded">
                                    {{ current.enabled ? "check" : "close" }}
                                </span>
                            </span>
                        </span>
                    </div>

                    <!-- Anything waiting on an answer comes first: it is the
                         only thing here that is blocking something. -->
                    <template v-if="current.pending.length">
                        <div class="caption">Waiting on you</div>
                        <div class="prompt" v-for="prompt in current.pending" :key="prompt.id">
                            <div class="what">
                                <div class="headline">{{ prompt.name || shortName(prompt) }}</div>
                                <div class="detail path">{{ prompt.exe }}</div>
                                <div v-if="prompt.dest" class="detail">→ {{ prompt.dest }}</div>
                            </div>
                            <div class="choices">
                                <button class="choice allow" @click="answer(prompt, 'allow', true)">
                                    Always allow
                                </button>
                                <button class="choice" @click="answer(prompt, 'allow', false)">Once</button>
                                <button class="choice deny" @click="answer(prompt, 'deny', true)">
                                    Deny
                                </button>
                            </div>
                        </div>
                    </template>

                    <div class="caption">Remembered</div>
                    <input v-model="filter" class="field" type="text" placeholder="Filter rules…" />

                    <div class="rules">
                        <div v-for="rule in rules" :key="rule.exe" class="rule">
                            <div class="what">
                                <div class="name">{{ shortName(rule) }}</div>
                                <div class="detail path">{{ rule.exe }}</div>
                            </div>
                            <div class="choices">
                                <button
                                    class="choice"
                                    :class="{ on: rule.action === 'allow' }"
                                    @click="setRule(rule, 'allow')"
                                >
                                    Allow
                                </button>
                                <button
                                    class="choice"
                                    :class="{ on: rule.action === 'deny' }"
                                    @click="setRule(rule, 'deny')"
                                >
                                    Deny
                                </button>
                                <button class="ghost" title="Forget" @click="forget(rule)">
                                    <span class="glyph material-symbols-rounded">delete</span>
                                </button>
                            </div>
                        </div>
                        <div v-if="!rules.length" class="detail">
                            {{ filter ? "Nothing matches" : "No rules yet" }}
                        </div>
                    </div>
                </template>
            </section>

            <!-- ---- what runs at login ------------------------------------ -->
            <section v-else-if="tab === 'startup'" class="body">
                <div class="caption">
                    {{ startup.length }} entr{{ startup.length === 1 ? "y" : "ies" }}
                </div>

                <div class="rules">
                    <div v-for="entry in startup" :key="`${entry.source}-${entry.key}`" class="rule">
                        <div class="what">
                            <div class="name">{{ entry.name }}</div>
                            <div class="detail path">{{ entry.exec || entry.key }}</div>
                        </div>
                        <div class="choices">
                            <span
                                class="switch"
                                :class="{ on: entry.enabled }"
                                @click="toggleStartup(entry)"
                            >
                                <span class="knob">
                                    <span class="material-symbols-rounded">
                                        {{ entry.enabled ? "check" : "close" }}
                                    </span>
                                </span>
                            </span>
                            <button class="ghost" title="Remove" @click="removeStartup(entry)">
                                <span class="glyph material-symbols-rounded">delete</span>
                            </button>
                        </div>
                    </div>
                    <div v-if="!startup.length" class="detail">Nothing starts with the session</div>
                </div>

                <div class="caption">Add one</div>
                <form class="add" @submit.prevent="addStartup">
                    <input v-model="newName" class="field" type="text" placeholder="Name" />
                    <input v-model="newExec" class="field" type="text" placeholder="Command" />
                    <button class="choice" type="submit">Add</button>
                </form>
            </section>

            <!-- ---- the feature modes ------------------------------------- -->
            <section v-else class="body">
                <div class="caption">Modes</div>
                <div class="modes">
                    <div v-for="mode in modes" :key="mode.id" class="mode" @click="toggleMode(mode)">
                        <span class="glyph material-symbols-rounded">{{ describe(mode.id).glyph }}</span>
                        <div class="what">
                            <div class="name">{{ describe(mode.id).name }}</div>
                            <div class="detail">{{ describe(mode.id).detail }}</div>
                        </div>
                        <span class="switch" :class="{ on: mode.enabled }">
                            <span class="knob">
                                <span class="material-symbols-rounded">
                                    {{ mode.enabled ? "check" : "close" }}
                                </span>
                            </span>
                        </span>
                    </div>
                    <div v-if="!modes.length" class="detail">Nothing to switch on this machine</div>
                </div>
            </section>
        </div>
    </div>
</template>

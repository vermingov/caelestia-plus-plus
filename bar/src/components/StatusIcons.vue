<script setup>
import { invoke } from "@tauri-apps/api/core";
import { computed, inject } from "vue";

const props = defineProps({
    snapshot: { type: Object, required: true },
    keyboard: { type: Object, default: () => ({ layout: "", capsLock: false, numLock: false }) },
    bluetooth: { type: Object, default: () => ({ powered: false, connected: 0 }) },
    // The power profile, which stands in for the battery slot on a machine
    // that has no battery.
    profile: { type: String, default: "" },
    // Which glyphs the row carries. The shell's bar reads the same flags from
    // shell.json; hard-coding the set here would mean a config that turns the
    // microphone on turns it on in one bar and not the other.
    status: {
        type: Object,
        default: () => ({
            showNetwork: true,
            showBluetooth: true,
            showBattery: true,
            showAudio: false,
            showMicrophone: false,
            showKbLayout: false,
            showLockStatus: false
        })
    }
});

const popout = inject("popout");

const network = computed(() => props.snapshot.network);
const battery = computed(() => props.snapshot.battery);
const volume = computed(() => props.snapshot.volume);
const microphone = computed(() => props.snapshot.microphone);

const volumeGlyph = computed(() => {
    if (!volume.value || volume.value.muted) return "volume_off";
    if (volume.value.level > 55) return "volume_up";
    if (volume.value.level > 0) return "volume_down";
    return "volume_mute";
});

const micGlyph = computed(() => (microphone.value?.muted ? "mic_off" : "mic"));

// Glyphs, not words: these are read at a glance or not at all.
const networkGlyph = computed(() => {
    if (network.value.kind === "ethernet") return "lan";
    if (network.value.kind !== "wifi") return "wifi_off";
    const bars = network.value.strength;
    if (bars > 70) return "network_wifi";
    if (bars > 45) return "network_wifi_3_bar";
    if (bars > 20) return "network_wifi_2_bar";
    return "network_wifi_1_bar";
});

const bluetoothGlyph = computed(() => {
    if (!props.bluetooth.powered) return "bluetooth_disabled";
    if (props.bluetooth.connected > 0) return "bluetooth_connected";
    return "bluetooth";
});

// With no battery the slot shows the power profile instead, which is what
// the shell's bar does on a desktop.
const batteryGlyph = computed(() => {
    if (!battery.value) return profileGlyph.value;
    if (battery.value.charging) return "battery_charging_full";
    const level = battery.value.level;
    if (level > 90) return "battery_full";
    if (level > 60) return "battery_5_bar";
    if (level > 40) return "battery_3_bar";
    if (level > 20) return "battery_2_bar";
    return "battery_alert";
});

// A machine with no battery still has a power profile, and that is what the
// slot shows instead — the same thing the shell's bar does.
const profileGlyph = computed(() => {
    switch (props.profile) {
        case "power-saver":
            return "energy_savings_leaf";
        case "performance":
            return "rocket_launch";
        default:
            return "balance";
    }
});

// Flat, not charging, and nobody has noticed: that is the one state on this
// bar that has earned a colour.
const batteryClass = computed(() => {
    if (!battery.value || battery.value.charging) return "";
    if (battery.value.level <= 10) return "alert";
    if (battery.value.level <= 25) return "warn";
    return "";
});
</script>

<template>
    <!-- The lock keys, which take no room at all until one of them is on, and
         only when the config asks for them at all. -->
    <div v-if="status.showLockStatus" class="locks">
        <Transition name="lock">
            <span v-if="keyboard.capsLock" class="glyph material-symbols-rounded">
                keyboard_capslock_badge
            </span>
        </Transition>
        <Transition name="lock">
            <span v-if="keyboard.numLock" class="glyph material-symbols-rounded">looks_one</span>
        </Transition>
    </div>

    <div v-if="status.showKbLayout && keyboard.layout" class="pill layout" :title="keyboard.layout">
        <Transition name="swap" mode="out-in">
            <span :key="keyboard.layout" class="value">{{ keyboard.layout }}</span>
        </Transition>
    </div>

    <!-- The readouts, in the order the shell's own row has them. -->
    <div class="section">
        <div
            v-if="status.showMicrophone && microphone"
            class="pill button icon-only"
            :class="{ lit: microphone.muted }"
            @click="invoke('mic_mute')"
            @mouseenter="popout.open('microphone', $event)"
            @mouseleave="popout.close()"
        >
            <Transition name="swap" mode="out-in">
                <span :key="micGlyph" class="glyph material-symbols-rounded">{{ micGlyph }}</span>
            </Transition>
        </div>

        <div
            v-if="status.showAudio && volume"
            class="pill button"
            @click="invoke('mute')"
            @wheel.prevent.stop="invoke('volume', { delta: $event.deltaY > 0 ? -5 : 5 })"
            @mouseenter="popout.open('volume', $event)"
            @mouseleave="popout.close()"
        >
            <Transition name="swap" mode="out-in">
                <span :key="volumeGlyph" class="glyph material-symbols-rounded">{{ volumeGlyph }}</span>
            </Transition>
            <span class="value">{{ volume.muted ? "—" : `${volume.level}%` }}</span>
        </div>

        <div
            v-if="status.showNetwork"
            class="pill button icon-only"
            @mouseenter="popout.open('network', $event)"
            @mouseleave="popout.close()"
        >
            <!-- Keyed on the glyph, so a link coming up or signal dropping a
                 bar is a change you can see happen rather than one you notice
                 later. -->
            <Transition name="swap" mode="out-in">
                <span :key="networkGlyph" class="glyph material-symbols-rounded">{{ networkGlyph }}</span>
            </Transition>
        </div>

        <div
            v-if="status.showBluetooth"
            class="pill button icon-only"
            :class="{ dim: !bluetooth.powered }"
            @mouseenter="popout.open('bluetooth', $event)"
            @mouseleave="popout.close()"
        >
            <Transition name="swap" mode="out-in">
                <span :key="bluetoothGlyph" class="glyph material-symbols-rounded">{{ bluetoothGlyph }}</span>
            </Transition>
        </div>

        <div
            v-if="status.showBattery"
            class="pill button icon-only"
            :class="batteryClass"
            @mouseenter="popout.open('battery', $event)"
            @mouseleave="popout.close()"
        >
            <Transition name="swap" mode="out-in">
                <span :key="batteryGlyph" class="glyph material-symbols-rounded">{{ batteryGlyph }}</span>
            </Transition>
        </div>
    </div>
</template>

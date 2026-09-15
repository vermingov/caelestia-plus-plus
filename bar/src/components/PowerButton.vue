<script setup>
import { invoke } from "@tauri-apps/api/core";
</script>

<template>
    <!-- The one control on the bar that ends the session, so it is the one
         thing drawn in the alert colour and given no popout to linger over. -->
    <div class="pill button icon-only danger" title="Session" @click="invoke('session')">
        <span class="glyph material-symbols-rounded">power_settings_new</span>
    </div>
</template>

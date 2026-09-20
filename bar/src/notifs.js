import { invoke } from "@tauri-apps/api/core";
import { createApp } from "vue";

import Notifs from "./Notifs.vue";
import "./style.css";
import "./notifs.css";

// This surface is transparent and usually empty, so a page that has fallen
// over looks the same as one with nothing to say. Anything that goes wrong is
// sent to the bar's log, which the shell keeps.
const log = message => invoke("notifs_log", { message }).catch(() => {});
window.addEventListener("error", event => log(`error: ${event.message} at ${event.filename}:${event.lineno}`));
window.addEventListener("unhandledrejection", event => log(`unhandled: ${event.reason?.stack ?? event.reason}`));

const app = createApp(Notifs);
app.config.errorHandler = (error, _instance, info) => log(`vue (${info}): ${error?.stack ?? error}`);
app.mount("#app");

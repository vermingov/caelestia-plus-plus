import { invoke } from "@tauri-apps/api/core";

/**
 * Everything the notification pages ask of the server, by name.
 *
 * One place, so that a component says `close(id)` and what that means on the
 * wire is decided here rather than in six templates.
 */

/** Takes a toast off the screen; the notification stays in the list. */
export const dismiss = id => invoke("notif_dismiss", { id });

/** Throws a notification away and tells whoever sent it. */
export const close = id => invoke("notif_close", { id });

/** Throws away everything one application sent. */
export const closeApp = appName => invoke("notif_close_app", { appName });

export const clear = () => invoke("notif_clear");

/** Presses one of the sender's own buttons. */
export const act = (id, action) => invoke("notif_action", { id, action });

export const setDnd = on => invoke("notif_dnd", { on });

/** Stops a toast's clock while the pointer rests on it, and restarts it. */
export const hold = (id, held) => invoke("notif_hold", { id, held });

export const openCentre = hover => invoke("notif_centre", { open: true, hover });
export const shutCentre = () => invoke("notif_centre", { open: false });

export const openLink = url => invoke("notif_open_link", { url });
export const copy = text => invoke("notif_copy", { text });

/** Where the pointer is on this surface, from the compositor. */
export const pointer = () => invoke("notifs_pointer");

/** Which boxes of this surface accept the pointer. */
export const reach = boxes => invoke("notifs_reach", { boxes });

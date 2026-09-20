import { invoke } from "@tauri-apps/api/core";

/**
 * Tells the backend whether anything is drawn below the strip.
 *
 * While nothing is, the backend repaints the strip alone instead of the whole
 * surface, which is most of what a frame costs it. It cannot see the page, so
 * it has to be told — and told from here, by the transitions themselves,
 * because only they know when a panel has finished leaving rather than begun.
 */

// How long the frame that no longer has the panel in it gets to reach the
// screen before the rest of the surface stops being repainted. It is a frame
// or two behind the hook that reports it; a panel cut off early would stay
// on screen as a ghost until the next one opened.
const LAST_FRAME = 150;

// A panel can start to enter while another is still leaving.
let hanging = 0;
let settling = null;

/** For a panel's `before-enter`. */
export function opens() {
    clearTimeout(settling);
    hanging += 1;
    if (hanging === 1) invoke("overhang", { open: true });
}

/** For its `after-leave`. */
export function closed() {
    hanging = Math.max(0, hanging - 1);
    if (hanging > 0) return;
    settling = setTimeout(() => invoke("overhang", { open: false }), LAST_FRAME);
}

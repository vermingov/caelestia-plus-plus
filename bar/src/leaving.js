import { invoke } from "@tauri-apps/api/core";

/**
 * Calls `onLeave` once the pointer is no longer over any of `boxes()`.
 *
 * Neither the events nor the engine's hover state can answer this on a layer
 * surface: the surface accepts the pointer only inside the strip and whatever
 * panel is open, so a pointer that crosses out of that region is simply gone —
 * the element it was over may never see a `mouseleave`, no further events
 * arrive, and the `:hover` it is still carrying is stale for the same reason.
 *
 * So the compositor is asked instead. It is the one source that is never
 * wrong, and at this rate it is a single socket round trip.
 *
 * `locate` is how the pointer is found. The default measures from the bar's
 * own surface; a page drawn on a different one passes its own.
 *
 * Returns a function that stops watching.
 */
export function whenPointerLeaves(
    boxes,
    onLeave,
    { every = 160, slack = 6, locate = () => invoke("pointer") } = {}
) {
    const timer = setInterval(async () => {
        const rects = boxes().filter(Boolean);
        if (!rects.length) return;

        const position = await locate();
        // No compositor to ask: the events are all there is, and they have
        // already had their chance.
        if (!position) return;

        const [x, y] = position;
        const over = rects.some(
            box =>
                x >= box.left - slack &&
                x <= box.right + slack &&
                y >= box.top - slack &&
                y <= box.bottom + slack
        );
        if (!over) onLeave();
    }, every);

    return () => clearInterval(timer);
}

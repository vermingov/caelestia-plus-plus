import { inject, onBeforeUnmount, ref } from "vue";

/**
 * Dragging something sideways to throw it away, and up or down to fold it.
 *
 * Three things here are borrowed from Lorenzo Cabra's drag blocks at
 * bencho.dev (MIT), each of which he found the hard way:
 *
 * The drag is followed on the window, not on the element. Pointer capture is
 * best-effort, and on a layer surface the pointer is outside the input region
 * for most of a swipe; a release has to end the drag wherever it lands.
 *
 * The listeners are bound at the press, not from a watcher. A fast flick puts
 * its first move in the gap before a watcher would run, and that first move
 * is the one that sets the grab.
 *
 * The grab offset is taken at the first move rather than the press, so the
 * first frame of a drag asks for exactly the position the thing already has.
 */
export function useSwipe({ onThrow, onFold }) {
    // How far counts as thrown and how far counts as folded are the person's
    // to set in shell.json, and are read at each press so that changing them
    // does not need anything restarted.
    const config = inject("notifsConfig", null);
    let throwAt = 0.3;
    let foldAfter = 20;

    // How far it has been pulled, in pixels. The template turns this into a
    // transform; nothing else moves the element while a drag is live.
    const pull = ref(0);
    const dragging = ref(false);

    let grab = null;
    let width = 1;
    let folded = false;
    // A press that never travelled is a click, and the caller may want it.
    let travelled = false;
    let loose = null;

    function move(event) {
        if (grab === null) {
            grab = { x: event.clientX, y: event.clientY };
            return;
        }
        const dx = event.clientX - grab.x;
        const dy = event.clientY - grab.y;
        if (Math.abs(dx) > 4 || Math.abs(dy) > 4) travelled = true;

        // Mostly vertical and far enough: fold or unfold, once per gesture.
        if (!folded && Math.abs(dy) > foldAfter && Math.abs(dy) > Math.abs(dx) * 1.5) {
            folded = true;
            onFold?.(dy > 0);
        }
        pull.value = folded ? 0 : dx;
    }

    function up() {
        loose?.();
        dragging.value = false;
        const thrown = Math.abs(pull.value) > width * throwAt;
        if (thrown) {
            // Carried off in the direction it was going, then reported.
            pull.value = Math.sign(pull.value) * width * 1.4;
            onThrow?.();
        } else {
            pull.value = 0;
        }
    }

    function down(event) {
        if (event.button !== 0) return;
        throwAt = config?.value.clearThreshold ?? throwAt;
        foldAfter = config?.value.expandThreshold ?? foldAfter;
        grab = null;
        folded = false;
        travelled = false;
        width = event.currentTarget.offsetWidth || 1;
        dragging.value = true;

        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
        window.addEventListener("pointercancel", up);
        loose = () => {
            window.removeEventListener("pointermove", move);
            window.removeEventListener("pointerup", up);
            window.removeEventListener("pointercancel", up);
            loose = null;
        };
    }

    onBeforeUnmount(() => loose?.());

    return { pull, dragging, down, wasClick: () => !travelled };
}

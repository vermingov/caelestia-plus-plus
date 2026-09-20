import { onBeforeUnmount, ref } from "vue";

/**
 * "now", "4m", "2h", "3d": how long ago something happened, as short as the
 * shell's own said it.
 */
export function ago(time, now) {
    const minutes = Math.floor((now - time) / 60000);
    if (minutes < 1) return "now";
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);
    if (days > 0) return `${days}d`;
    if (hours > 0) return `${hours}h`;
    return `${minutes}m`;
}

/**
 * One clock for every timestamp on the page.
 *
 * The shell gave each notification a timer of its own, three hundred of them
 * ticking at different rates. Nothing here is more precise than a minute, so
 * one reading shared by everything that shows a time is all it takes.
 */
export function useNow(every = 20000) {
    const now = ref(Date.now());
    const timer = setInterval(() => (now.value = Date.now()), every);
    onBeforeUnmount(() => clearInterval(timer));
    return now;
}

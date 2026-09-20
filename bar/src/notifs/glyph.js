/**
 * The icon for a notification that brought none of its own, chosen from what
 * it says. The shell's list, kept word for word: these are the notifications
 * this desktop itself sends, and they were the ones it was written for.
 */
const BY_WORD = [
    ["reboot", "restart_alt"],
    ["recording", "screen_record"],
    ["battery", "power"],
    ["screenshot", "screenshot_monitor"],
    ["welcome", "waving_hand"],
    ["time", "schedule"],
    ["a break", "schedule"],
    ["installed", "download"],
    ["update", "update"],
    ["unable to", "deployed_code_alert"],
    ["profile", "person"],
    ["file", "folder_copy"]
];

export const CRITICAL = 2;
export const LOW = 0;

export function fallbackGlyph(summary, urgency) {
    const text = (summary ?? "").toLowerCase();
    const match = BY_WORD.find(([word]) => text.includes(word));
    if (match) return match[1];
    return urgency === CRITICAL ? "release_alert" : "chat";
}

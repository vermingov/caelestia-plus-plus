//! Game mode: the compositor stripped of everything that costs a frame.
//!
//! Animations, blur, shadows, gaps, rounding off and tearing allowed, for as
//! long as it is on. Set as keywords rather than written to the config, so
//! that turning it off is one reload and nothing of the user's is ever
//! rewritten — which is also how the shell it came from did it.

use crate::{hypr, tell};

/// What it turns off, and what it allows.
const WHILE_PLAYING: [(&str, &str); 8] = [
    ("animations:enabled", "0"),
    ("decoration:shadow:enabled", "0"),
    ("decoration:blur:enabled", "0"),
    ("decoration:rounding", "0"),
    ("general:gaps_in", "0"),
    ("general:gaps_out", "0"),
    ("general:border_size", "1"),
    ("general:allow_tearing", "1"),
];

/// Whether the compositor is stripped down now. Read from the compositor
/// rather than remembered: a reload for any other reason ends it, and
/// nothing says so.
pub fn enabled() -> bool {
    hypr::option("animations:enabled") == Some(0)
}

pub fn set(playing: bool) {
    if !playing {
        hypr::reload();
        return tell::said("Game mode off", "The compositor is itself again", "sports_esports");
    }
    for (option, value) in WHILE_PLAYING {
        hypr::keyword(option, value);
    }
    tell::said("Game mode on", "No animations, no blur, no gaps, tearing allowed", "sports_esports");
}

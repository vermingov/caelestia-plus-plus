//! `caelestia emoji` — the emoji and glyph picker on SUPER+Period.
//!
//! The list itself is a data file the Python CLI ships and refreshes from the
//! network; fetching stays over there. Picking from it is three pipes and has
//! no business waiting on an interpreter.

use crate::{paths, proc};

pub fn run(picker: bool) -> i32 {
    let Some(path) = paths::emoji_data_path() else {
        eprintln!("caelestia: no emoji data found — run `caelestia emoji -f` to fetch it");
        return 1;
    };
    let Ok(emojis) = std::fs::read(&path) else {
        eprintln!("caelestia: cannot read {}", path.display());
        return 1;
    };

    if !picker {
        use std::io::Write;
        let _ = std::io::stdout().write_all(&emojis);
        return 0;
    }

    let Some(chosen) = proc::pipe(
        "fuzzel",
        &["--dmenu", "--placeholder=Type to search emojis"],
        &emojis,
    ) else {
        return 0; // dismissed
    };

    // Each line is "<glyph> <name> <tags…>"; only the glyph is wanted, and
    // without the trailing newline fuzzel hands back.
    let text = String::from_utf8_lossy(&chosen);
    let Some(glyph) = text.split_whitespace().next() else { return 0 };
    proc::pipe("wl-copy", &[], glyph.as_bytes());
    0
}

//! `caelestia clipboard` — the history picker on SUPER+V.
//!
//! Three programs in a row: cliphist holds the history, fuzzel picks from it,
//! wl-copy puts the choice back. Cancelling the picker is an ordinary outcome,
//! not an error, so it exits quietly.

use crate::proc;

pub fn run(delete: bool) -> i32 {
    let Some(history) = proc::capture("cliphist", &["list"]) else {
        eprintln!("caelestia: cliphist could not read the clipboard history");
        return 1;
    };

    let picker_args: Vec<&str> = if delete {
        vec!["--dmenu", "--prompt=del > ", "--placeholder=Delete from clipboard"]
    } else {
        vec!["--dmenu", "--placeholder=Type to search clipboard"]
    };

    let Some(chosen) = proc::pipe("fuzzel", &picker_args, &history) else {
        return 0; // dismissed
    };
    if chosen.is_empty() {
        return 0;
    }

    if delete {
        proc::pipe("cliphist", &["delete"], &chosen);
        return 0;
    }

    match proc::pipe("cliphist", &["decode"], &chosen) {
        Some(entry) => {
            proc::pipe("wl-copy", &[], &entry);
            0
        }
        None => 1,
    }
}

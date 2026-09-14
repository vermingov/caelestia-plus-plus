//! caelestia — the command behind every keybind in this desktop.
//!
//! It exists for one reason: latency. The Python CLI it fronts spends about
//! 110 ms importing itself before it can decide what it was asked to do, and
//! that cost lands on every workspace toggle, every screenshot, every
//! clipboard pick, because those are keybinds a person is waiting on.
//! Everything here is the same work without the interpreter.
//!
//! It is a front-end, not a fork. Subcommands it implements it answers
//! itself; everything else — scheme, wallpaper, install, update — is handed
//! to the Python CLI with the arguments untouched, so there is exactly one
//! definition of what those do. The same goes for anything it fails to parse:
//! unknown flags are somebody else's to understand, not ours to guess at.

mod args;
mod clock;
mod cmd;
mod config;
mod hypr;
mod paths;
mod proc;
mod scheme;
mod sha256;

use std::os::unix::process::CommandExt;
use std::process::Command;

/// The CLI this one defers to. Overridable so the two can be compared, and
/// so a machine that installs it elsewhere still works.
fn python_cli() -> String {
    std::env::var("CAELESTIA_PYTHON_CLI").unwrap_or_else(|_| "/usr/bin/caelestia".to_string())
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let code = match args::parse(&argv) {
        Some(args::Command::Shell(a)) => cmd::shell::run(&a),
        Some(args::Command::Toggle(workspace)) => {
            cmd::toggle::run(&workspace);
            0
        }
        Some(args::Command::Clipboard { delete }) => cmd::clipboard::run(delete),
        Some(args::Command::Emoji { picker }) => cmd::emoji::run(picker),
        Some(args::Command::Screenshot(a)) => cmd::screenshot::run(&a),
        Some(args::Command::Record(a)) => cmd::record::run(&a),
        Some(args::Command::SchemeGet(a)) => cmd::scheme::get(&a),
        Some(args::Command::SchemeList(a)) => cmd::scheme::list(&a),
        None => hand_over(&argv),
    };
    std::process::exit(code);
}

/// Replace this process with the Python CLI. An exec rather than a spawn, so
/// the caller sees its exit code and its signals directly and there is no
/// second process in the tree for the 200 µs this took.
pub fn hand_over(argv: &[String]) -> i32 {
    let target = python_cli();

    // Installed over the top of the very CLI we defer to, every call would
    // recurse until the machine ran out of processes. Refuse instead.
    if same_file(&target, std::env::current_exe().ok().as_deref()) {
        eprintln!(
            "caelestia: {target} is this program — install the Rust CLI somewhere earlier on PATH \
             (e.g. /usr/local/bin) and leave the Python one where it is"
        );
        return 127;
    }

    let error = Command::new(&target).args(argv).exec();
    eprintln!("caelestia: cannot run {target}: {error}");
    127
}

fn same_file(path: &str, other: Option<&std::path::Path>) -> bool {
    let Some(other) = other else { return false };
    match (std::fs::canonicalize(path), std::fs::canonicalize(other)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

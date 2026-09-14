//! caelestia-tools — the small programs the shell shells out to.
//!
//! Each of these was a Python script under assets/. They are short, they run
//! at startup or on a keypress, and every one of them paid 25 to 50 ms of
//! interpreter start to do a few file reads. One of them never exited at all
//! and held an interpreter resident for the life of the session.
//!
//! The shell prefers this binary and falls back to the scripts when it is
//! absent, so a checkout without a Rust toolchain still works exactly as it
//! did.

mod cmd;
mod difflib;
mod jsonval;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (command, rest) = match argv.split_first() {
        Some((command, rest)) => (command.as_str(), rest),
        None => {
            usage();
            std::process::exit(2);
        }
    };

    let code = match command {
        // What this build can do, one line each. The shell asks before using
        // the binary, so an old one falls back per tool rather than failing.
        "list" => {
            for (name, _) in TOOLS {
                println!("{name}");
            }
            0
        }
        "gpus" => cmd::gpus::run(),
        "startup" => cmd::startup::run(rest),
        "egg-watch" => cmd::egg::run(rest),
        "hyprmod" => cmd::hyprmod::run(rest),
        "config-doctor" => cmd::doctor::run(rest),
        "seccomp" => cmd::seccomp::run(rest),
        "-h" | "--help" | "help" => {
            usage();
            0
        }
        other => {
            eprintln!("caelestia-tools: unknown command {other}");
            usage();
            2
        }
    };
    std::process::exit(code);
}

/// Every command this build answers to, with a line about each. `list`
/// prints the names, which is how the shell decides whether this binary is
/// new enough for a given tool — a checkout can move ahead of an installed
/// binary, and a missing subcommand must fall back to its script rather than
/// fail.
const TOOLS: [(&str, &str); 6] = [
    ("gpus", "list render-capable GPUs as JSON"),
    ("startup", "read and write the startup application list"),
    ("egg-watch", "watch for the easter egg's key sequence"),
    ("hyprmod", "the settings UI's bridge to the Hyprland config"),
    ("config-doctor", "diagnose and repair shell.json"),
    ("seccomp", "write the sandbox's seccomp filter"),
];

fn usage() {
    eprintln!("caelestia-tools <command>\n");
    for (name, what) in TOOLS {
        eprintln!("  {name:<14} {what}");
    }
    eprintln!("  {:<14} print the command names this build answers to", "list");
}

#[cfg(test)]
mod tests {
    use super::TOOLS;

    /// `list` is what the shell trusts to decide which tools this build has,
    /// so every name it prints must be one `main` actually dispatches — and
    /// every name `main` dispatches must be printed.
    #[test]
    fn the_listed_tools_are_the_dispatched_ones() {
        let source = include_str!("main.rs");
        let dispatch: Vec<&str> = source
            .lines()
            .filter_map(|line| line.trim().strip_prefix('"')?.split_once("\" =>"))
            .map(|(name, _)| name)
            // The help aliases share one arm, and `list` is not a tool.
            .filter(|name| !name.contains('"') && *name != "list")
            .collect();

        let listed: Vec<&str> = TOOLS.iter().map(|(name, _)| *name).collect();
        assert_eq!(dispatch, listed, "the tool list and the dispatch disagree");
        for (name, what) in TOOLS {
            assert!(!what.is_empty(), "{name} has no description");
        }
    }
}

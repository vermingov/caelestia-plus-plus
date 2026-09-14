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
        "gpus" => cmd::gpus::run(),
        "startup" => cmd::startup::run(rest),
        "egg-watch" => cmd::egg::run(rest),
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

fn usage() {
    eprintln!(
        "caelestia-tools <command>\n\
         \n\
         \x20 gpus   list render-capable GPUs as JSON"
    );
}

//! `caelestia shell` — start the shell, message it, or read its log.
//!
//! Every call here is a thin wrapper around `qs`, which is the point: the
//! Python version paid an interpreter start before it could even spell the
//! word `qs`, and this is on the path of every IPC call the bar's own
//! keybinds make.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

pub struct Args {
    pub message: Vec<String>,
    pub daemon: bool,
    pub show: bool,
    pub log: bool,
    pub kill: bool,
    pub log_rules: Option<String>,
}

pub fn run(args: &Args) -> i32 {
    if args.show {
        return print_filtered(&["ipc", "show"]);
    }
    if args.log {
        let mut qs_args = vec!["log".to_string()];
        if let Some(rules) = &args.log_rules {
            qs_args.push("-r".to_string());
            qs_args.push(rules.clone());
        }
        let borrowed: Vec<&str> = qs_args.iter().map(String::as_str).collect();
        return print_filtered(&borrowed);
    }
    if args.kill {
        return qs(&["kill"]).map(|_| 0).unwrap_or(1);
    }
    if !args.message.is_empty() {
        let mut call = vec!["ipc", "call"];
        call.extend(args.message.iter().map(String::as_str));
        return print_filtered(&call);
    }
    start(args)
}

fn qs(args: &[&str]) -> Option<String> {
    let out = Command::new("qs")
        .args(["-c", "caelestia"])
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    out.status.success().then_some(text)
}

fn print_filtered(args: &[&str]) -> i32 {
    match qs(args) {
        Some(text) => {
            for line in text.lines() {
                if keep(line) {
                    println!("{line}");
                }
            }
            0
        }
        None => 1,
    }
}

/// How many heap arenas the shell gets.
///
/// jemalloc defaults to four per core, and each one keeps its own cache of
/// freed pages. On a sixteen-core machine that is sixty-four caches for a
/// process whose live heap is a fraction of what they retain between them —
/// measured here at 481 MiB resident against 280 MiB with this set, for the
/// same shell doing the same work.
///
/// Four rather than fewer: two saves another eight megabytes and costs twice
/// the idle CPU in contention, which is the wrong trade for a process that
/// spends its life waiting.
const ARENAS: &str = "narenas:4";

fn start(args: &Args) -> i32 {
    let mut cmd = Command::new("qs");
    cmd.args(["-c", "caelestia", "-n"]);

    // Set here rather than in the shell's own config: jemalloc reads this
    // once, before `main`, so a pragma inside the QML would be far too late.
    // Anything the caller already set wins, so this stays overridable.
    if std::env::var_os("MALLOC_CONF").is_none() {
        cmd.env("MALLOC_CONF", ARENAS);
    }
    if let Some(rules) = &args.log_rules {
        cmd.args(["--log-rules", rules]);
    }

    if args.daemon {
        // `qs -d` detaches itself, so waiting on it costs nothing and keeps
        // its exit code meaningful when it fails to start at all.
        cmd.arg("-d");
        return cmd.status().ok().and_then(|s| s.code()).unwrap_or(1);
    }

    // In the foreground the shell's log is ours to print, minus the noise.
    let Ok(mut child) = cmd.stdout(Stdio::piped()).spawn() else {
        eprintln!("caelestia: cannot start qs");
        return 1;
    };
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if keep(&line) {
                println!("{line}");
            }
        }
    }
    child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
}

/// One warning per cached image the shell could not open is not news; the
/// Python CLI filtered it out and so does this.
fn keep(line: &str) -> bool {
    let noise = format!(
        "Cannot open: file://{}/imagecache/",
        crate::paths::caelestia_cache_dir().display()
    );
    !line.contains(&noise)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_imagecache_warning_is_the_only_line_dropped() {
        let cache = crate::paths::caelestia_cache_dir();
        let noisy = format!("warning: Cannot open: file://{}/imagecache/abc.png", cache.display());
        assert!(!keep(&noisy));
        assert!(keep("anything else the shell says"));
        assert!(keep(""));
    }
}

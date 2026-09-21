//! `caelestia shell` — start the shell, message it, or read its log.
//!
//! Every call here is a thin wrapper around `qs`, which is the point: the
//! Python version paid an interpreter start before it could even spell the
//! word `qs`, and this is on the path of every IPC call the bar's own
//! keybinds make.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
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

/// Says `words` at the door of the shell that is running, and says whether
/// anything was listening. cae answers here; the QML shell has no door and
/// is asked over its own IPC instead.
pub fn knock(words: &[&str]) -> bool {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| std::path::PathBuf::from("/tmp"), std::path::PathBuf::from);
    let Ok(mut door) = UnixStream::connect(runtime.join("caelestia-shell.sock")) else { return false };
    door.write_all(format!("{}\n", words.join(" ")).as_bytes()).is_ok()
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

/// The daemon, as a command line for the compositor to run.
///
/// The shell hands its environment to everything it starts: the bar, and
/// through the bar's launcher every application. Started as a child of whoever
/// asked, that environment is the caller's. Over SSH, to update someone else's
/// machine, the caller has only what it exported by hand — the Wayland socket
/// and the Hyprland signature, which is what the shell refuses to draw without.
/// Nothing in the shell speaks X11, so the missing `DISPLAY` went unnoticed
/// until Steam would not start from the launcher while starting fine from a
/// terminal, and every restart after that ran from inside the old shell and
/// inherited the gap, update after update. A terminal is no better a parent:
/// its own variables, and any token in them, reach every application too.
///
/// Hyprland starts the shell at login, so a shell it starts now gets the
/// environment it would have had then, whoever is asking. `env` carries the one
/// variable that is ours to set, because nothing else of the caller's arrives.
fn for_the_compositor(qs: &[String], malloc_conf: &str) -> String {
    let mut command = vec!["env".to_string(), format!("MALLOC_CONF={malloc_conf}")];
    command.extend_from_slice(qs);
    crate::proc::shell_join(&command)
}

fn start(args: &Args) -> i32 {
    let mut qs: Vec<String> = ["qs", "-c", "caelestia", "-n"].map(String::from).to_vec();
    if let Some(rules) = &args.log_rules {
        qs.extend(["--log-rules".to_string(), rules.clone()]);
    }

    // Set here rather than in the shell's own config: jemalloc reads this
    // once, before `main`, so a pragma inside the QML would be far too late.
    // Anything the caller already set wins, so this stays overridable.
    let malloc_conf = std::env::var("MALLOC_CONF").unwrap_or_else(|_| ARENAS.to_string());

    if args.daemon {
        qs.push("-d".to_string());
        return start_daemon(&qs, &malloc_conf);
    }
    follow(&qs, &malloc_conf)
}

fn from_here(qs: &[String], malloc_conf: &str) -> Command {
    let mut cmd = Command::new(&qs[0]);
    cmd.args(&qs[1..]).env("MALLOC_CONF", malloc_conf);
    cmd
}

fn start_daemon(qs: &[String], malloc_conf: &str) -> i32 {
    if crate::hypr::exec(&for_the_compositor(qs, malloc_conf)) {
        return 0;
    }

    // No Hyprland answered, so it is started from here after all. `qs -d`
    // detaches itself, so waiting on it costs nothing and keeps its exit code
    // meaningful when it fails to start at all.
    from_here(qs, malloc_conf).status().ok().and_then(|s| s.code()).unwrap_or(1)
}

/// In the foreground the shell's log is ours to print, minus the noise — which
/// only a child of this process can give us, so this one is started from here.
fn follow(qs: &[String], malloc_conf: &str) -> i32 {
    let Ok(mut child) = from_here(qs, malloc_conf).stdout(Stdio::piped()).spawn() else {
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

    fn words(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn the_compositor_is_handed_the_same_command_line_a_login_runs() {
        let qs = words(&["qs", "-c", "caelestia", "-n", "-d"]);
        assert_eq!(
            for_the_compositor(&qs, ARENAS),
            "env MALLOC_CONF=narenas:4 qs -c caelestia -n -d"
        );
    }

    #[test]
    fn log_rules_reach_the_compositor_as_one_word() {
        let qs = words(&["qs", "-c", "caelestia", "-n", "--log-rules", "quickshell.*=true;qt.*=false", "-d"]);
        assert_eq!(
            for_the_compositor(&qs, "narenas:2,dirty_decay_ms:0"),
            "env MALLOC_CONF=narenas:2,dirty_decay_ms:0 qs -c caelestia -n --log-rules 'quickshell.*=true;qt.*=false' -d"
        );
    }
}

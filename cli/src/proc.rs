//! Running other programs, which is most of what this CLI does.
//!
//! Every helper here is deliberately thin: the point of the Rust front-end is
//! that it reaches `grim`, `fuzzel` or Hyprland in microseconds rather than
//! after an interpreter start, not that it reimplements them.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Run a command and collect its standard output.
pub fn capture(program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(program).args(args).stderr(Stdio::inherit()).output().ok()?;
    out.status.success().then_some(out.stdout)
}

pub fn capture_text(program: &str, args: &[&str]) -> Option<String> {
    String::from_utf8(capture(program, args)?).ok()
}

/// Feed `input` to a command and collect what it writes back. Used for every
/// `x | fuzzel | y` shape the picker subcommands are built from.
pub fn pipe(program: &str, args: &[&str], input: &[u8]) -> Option<Vec<u8>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .ok()?;
    // Written before the wait: a program that reads its input to the end
    // never exits while we are still holding the pipe open.
    child.stdin.take()?.write_all(input).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Run to completion, ignoring output. Returns whether it succeeded.
pub fn run(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Start a program that must outlive this one — an editor on a screenshot, a
/// recorder — in its own session, so it is not killed with the terminal or
/// the keybind that started it.
pub fn spawn_detached(program: &str, args: &[&str]) -> Option<std::process::Child> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut cmd);
    cmd.spawn().ok()
}

/// As above, but the program is handed its input on stdin (swappy reads the
/// image that way).
pub fn spawn_detached_with_input(program: &str, args: &[&str], input: &[u8]) -> Option<()> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    stdin.write_all(input).ok()?;
    drop(stdin); // the editor waits for end-of-input before drawing anything
    Some(())
}

fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        // setsid() in the child, between fork and exec: a new session means no
        // controlling terminal to be hung up on, matching what the Python CLI
        // asked for with start_new_session.
        cmd.pre_exec(|| {
            setsid();
            Ok(())
        });
    }
}

extern "C" {
    fn setsid() -> i32;
}

/// `notify-send`, which prints the id of the notification it posted, or the
/// action the user picked when the call carried `--action`.
pub fn notify(args: &[&str]) -> String {
    let mut full = vec!["-a", "caelestia++"];
    let offers_actions = args.iter().any(|arg| arg.starts_with("--action"));
    if offers_actions {
        full.extend_from_slice(&["-t", ACTION_EXPIRY_MS]);
    }
    full.extend_from_slice(args);

    if !offers_actions {
        return capture_text("notify-send", &full).map(|s| s.trim().to_string()).unwrap_or_default();
    }

    let Ok(child) = Command::new("notify-send").args(&full).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()
    else {
        return String::new();
    };
    wait_bounded(child, ACTION_WINDOW).unwrap_or_default().trim().to_string()
}

/// How long a notification that offers actions stays up, and how long its
/// `notify-send` is waited for — a little longer, so a click at the last
/// moment still lands.
const ACTION_EXPIRY_MS: &str = "60000";
const ACTION_WINDOW: Duration = Duration::from_secs(65);

/// Waits for `child` until the deadline and hands back what it printed; past
/// the deadline it is killed and reaped, and the answer is nothing.
///
/// `notify-send --action` only exits on a click or when the notification
/// server says the notification closed, and a server that lets one expire
/// without saying so leaves it waiting for good. This process is detached
/// from the shell, so nothing else would ever collect either of them: every
/// recording and every screenshot left a pair behind until the next reboot.
fn wait_bounded(mut child: Child, window: Duration) -> Option<String> {
    let deadline = Instant::now() + window;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut printed = String::new();
    child.stdout.take()?.read_to_string(&mut printed).ok()?;
    Some(printed)
}

pub fn close_notification(id: &str) {
    if id.is_empty() {
        return;
    }
    run(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest=org.freedesktop.Notifications",
            "--object-path=/org/freedesktop/Notifications",
            "--method=org.freedesktop.Notifications.CloseNotification",
            id,
        ],
    );
}

/// Whether a program exists on PATH, the check the toggle config makes before
/// spawning an app that may not be installed.
pub fn which(program: &str) -> bool {
    if program.contains('/') {
        return std::path::Path::new(program).is_file();
    }
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(program)))
}

fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Quote a command the way `shlex.join` does, because it is handed to
/// Hyprland as one string and re-split by a shell on the way to exec.
pub fn shell_join(parts: &[String]) -> String {
    parts.iter().map(|p| quote(p)).collect::<Vec<_>>().join(" ")
}

fn quote(word: &str) -> String {
    let safe = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if safe {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_real_program_and_not_an_invented_one() {
        assert!(which("sh"), "sh is on every PATH this runs on");
        assert!(!which("definitely-not-a-real-program-42"));
        assert!(which("/bin/sh"));
        assert!(!which("/bin/definitely-not-real"));
    }

    #[test]
    fn quotes_only_what_a_shell_would_mangle() {
        assert_eq!(shell_join(&["foot".into(), "-a".into(), "btop".into()]), "foot -a btop");
        assert_eq!(
            shell_join(&["fish".into(), "-C".into(), "exec btop".into()]),
            "fish -C 'exec btop'"
        );
        assert_eq!(shell_join(&["it's".into()]), r#"'it'"'"'s'"#);
        assert_eq!(shell_join(&["".into()]), "''");
    }

    #[test]
    fn a_child_that_outlives_the_window_is_killed_and_reaped() {
        let child = Command::new("sleep").arg("30").stdout(Stdio::piped()).spawn().unwrap();
        let pid = child.id();
        let started = Instant::now();

        assert_eq!(wait_bounded(child, Duration::from_millis(300)), None);

        assert!(started.elapsed() >= Duration::from_millis(300), "waited out the window first");
        assert!(started.elapsed() < Duration::from_secs(5), "then gave up promptly");
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists(), "no process left behind");
    }

    #[test]
    fn a_child_that_answers_in_time_is_heard() {
        let child = Command::new("echo").arg("watch").stdout(Stdio::piped()).spawn().unwrap();
        assert_eq!(wait_bounded(child, Duration::from_secs(5)).as_deref().map(str::trim), Some("watch"));
    }
}

//! The children that run for as long as the bar does: `pw-record` for the
//! visualiser, `pactl` for the volume.
//!
//! Both are read through a pipe, and both need the two things `std` does not
//! offer: a read that gives up, and a child that cannot outlive its parent.
//!
//! And the opposite kind: what the shell starts for somebody, which must
//! outlive it, and must not be left for it to reap.

use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Makes the child die with the bar, whatever the bar dies of.
///
/// Nothing here runs when the bar is terminated, and a child left alone only
/// finds out by writing to the closed pipe. One with nothing to write — no
/// samples reaching it, no events to report — never finds out: one more
/// orphan for every restart. The signal follows the thread that started the
/// child, so that thread has to be one that lasts.
pub fn bind_to_parent(command: &mut Command) {
    // SAFETY: `prctl` is async-signal-safe and touches only the child.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
}

/// Starts `argv` the way a desktop starts an app: in a scope of its own
/// under `app.slice`, detached, with nothing left over for this process to
/// reap. `name` says what it is, for the scope's name.
///
/// The shell is a service, and systemd stops a service by ending every
/// process in it. An app started from here used to be one of those, so a
/// restart of the shell — an update, or the shell falling over — ended every
/// app it had started; the updater, run from the settings, ended itself with
/// the restart it asks for. An app that moves itself into a scope of its own,
/// as Chromium does, still left behind everything it had forked by then.
///
/// Where no user service manager is running there is no scope to be had, and
/// the app is only detached, as it always was.
pub fn launch(name: &str, argv: &[&str]) {
    let mut command = Command::new("setsid");
    command.arg("-f");
    if let Some(unit) = scope_for(name) {
        command.args(["systemd-run", "--user", "--scope", "--quiet", "--collect", "--slice=app.slice"]);
        command.arg(format!("--unit={unit}")).arg("--");
    }
    command.args(argv);
    // What the shell's own unit hands it, which is not the app's to have.
    for unit_only in ["INVOCATION_ID", "JOURNAL_STREAM", "SYSTEMD_EXEC_PID", "MANAGERPID", "NOTIFY_SOCKET"] {
        command.env_remove(unit_only);
    }
    detached(&mut command);
}

/// Runs `command`, which starts with `setsid -f`, with nothing to say and
/// nothing to hear. `setsid -f` forks and returns at once, so waiting for it
/// is what reaps it; a child that is only spawned stays a zombie of the
/// shell's until the shell exits.
pub fn detached(command: &mut Command) {
    let _ = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// `app-caelestia-<name>-<n>.scope`, the name the desktop's own launchers
/// give an app's scope, or nothing where there is no manager to ask.
fn scope_for(name: &str) -> Option<String> {
    static MANAGED: OnceLock<bool> = OnceLock::new();
    let managed = MANAGED.get_or_init(|| {
        std::env::var_os("XDG_RUNTIME_DIR")
            .is_some_and(|runtime| std::path::Path::new(&runtime).join("systemd/private").exists())
    });
    if !managed {
        return None;
    }
    // The process and a count, which no other shell's launches can spell: an
    // app outlives the shell that started it, and its scope keeps the name
    // for as long as it runs. The count is fixed-width so that the two
    // cannot run into each other.
    static STARTED: AtomicU32 = AtomicU32::new(0);
    let nth = STARTED.fetch_add(1, Ordering::Relaxed);
    Some(format!("app-caelestia-{}-{:x}{nth:08x}.scope", escape(name), std::process::id()))
}

/// A name as systemd writes one into a unit's: the characters a unit name
/// may not hold, and the dash that separates its parts, as `\xNN`. A path is
/// its last part, and only the start of a long name is kept: a unit's name
/// is at most 255 bytes, and one that is longer is refused along with the
/// app it was for.
fn escape(name: &str) -> String {
    const KEPT: usize = 48;
    let name = name.rsplit('/').find(|part| !part.is_empty()).unwrap_or_default();
    let mut escaped = String::with_capacity(name.len());
    for byte in name.bytes().take(KEPT) {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':') {
            escaped.push(byte as char);
        } else {
            escaped.push_str(&format!("\\x{byte:02x}"));
        }
    }
    if escaped.is_empty() { "app".to_string() } else { escaped }
}

/// Whether `pipe` has something to read, waiting up to `patience` for it, or
/// for as long as it takes when given none.
///
/// A pipe whose other end has closed counts as readable: the read that
/// follows is what reports it.
pub fn readable(pipe: &impl AsRawFd, patience: Option<Duration>) -> bool {
    let timeout = patience.map_or(-1, |wait| wait.as_millis() as libc::c_int);
    let mut descriptor = libc::pollfd { fd: pipe.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    // SAFETY: one initialised `pollfd`, which outlives the call.
    unsafe { libc::poll(&mut descriptor, 1, timeout) > 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_written_the_way_systemd_escapes_one() {
        assert_eq!(escape("org.mozilla.firefox"), "org.mozilla.firefox");
        assert_eq!(escape("fluxer-canary"), "fluxer\\x2dcanary");
        assert_eq!(escape("my app 2"), "my\\x20app\\x202");
        assert_eq!(escape("/usr/bin/foot"), "foot");
        assert_eq!(escape(""), "app");
        assert_eq!(escape("/"), "app");
        // However long the name, the unit's stays well inside 255 bytes.
        assert!(escape(&"-".repeat(300)).len() <= 4 * 48);
    }
}

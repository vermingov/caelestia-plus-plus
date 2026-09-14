//! Freezing, releasing and killing another process.
//!
//! The freeze is what buys the user time to answer: a payload waiting for its
//! operator's next command is stopped mid-flight and cannot act while the
//! prompt is up. Releasing has to be reliable for the same reason the freeze
//! does — a process left stopped forever is a hang the user cannot explain.

/// Linux signal numbers. Identical on every architecture this runs on; the
/// outliers (alpha, mips, parisc) are not desktop targets.
const SIGKILL: i32 = 9;
const SIGCONT: i32 = 18;
const SIGSTOP: i32 = 19;

/// Stop a process. False means it was already gone, which is the common race:
/// short-lived processes exit while they are being classified.
pub fn freeze(pid: u32) -> bool {
    send(pid, SIGSTOP)
}

pub fn release(pid: u32) {
    send(pid, SIGCONT);
}

/// Kill a process and the group it leads. Payloads spawn children — a shell
/// that has already forked its own helpers — so killing the pid alone can
/// leave the interesting half running. SIGCONT goes first because a stopped
/// process cannot die of SIGKILL until it is scheduled again.
pub fn terminate(pid: u32) {
    // Never signal our own group: that would take the daemon down with the
    // process it is killing. Group 1 and 0 are likewise never ours to kill.
    let group = group_of(pid).filter(|&g| g > 1 && Some(g) != group_of(std::process::id()));

    for sig in [SIGCONT, SIGKILL] {
        let group_died = group.is_some_and(|g| unsafe { killpg(g, sig) } == 0);
        if !group_died {
            send(pid, sig);
        }
    }
}

fn send(pid: u32, sig: i32) -> bool {
    unsafe { kill(pid as i32, sig) == 0 }
}

fn group_of(pid: u32) -> Option<i32> {
    let pgid = unsafe { getpgid(pid as i32) };
    (pgid > 0).then_some(pgid)
}

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn killpg(pgrp: i32, sig: i32) -> i32;
    fn getpgid(pid: i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    fn sleeper() -> Child {
        Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn a process to freeze")
    }

    /// The `stat` field that says running, sleeping or stopped.
    fn state(pid: u32) -> Option<char> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat[stat.rfind(')')? + 1..]
            .split_ascii_whitespace()
            .next()?
            .chars()
            .next()
    }

    /// Signals are delivered asynchronously, so the state change lands shortly
    /// after the call rather than during it.
    fn wait_for(pid: u32, want: char) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if state(pid) == Some(want) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn a_process_freezes_and_comes_back() {
        let mut child = sleeper();
        let pid = child.id();

        assert!(freeze(pid), "the freeze reached it");
        assert!(wait_for(pid, 'T'), "stopped, not merely signalled");

        release(pid);
        assert!(wait_for(pid, 'S'), "running again after the release");

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn terminating_a_frozen_process_still_kills_it() {
        let mut child = sleeper();
        let pid = child.id();
        assert!(freeze(pid));
        assert!(wait_for(pid, 'T'));

        terminate(pid);
        // The parent has to reap it before /proc/<pid> disappears, so the
        // observable outcome is the wait, not the empty directory.
        let status = child.wait().expect("reaped");
        assert!(!status.success(), "killed rather than exited cleanly");
    }

    #[test]
    fn signalling_a_process_that_is_already_gone_is_not_fatal() {
        let mut child = sleeper();
        let pid = child.id();
        let _ = child.kill();
        let _ = child.wait();

        assert!(!freeze(pid), "nothing there to freeze");
        release(pid); // must not panic
        terminate(pid);
    }

    #[test]
    fn our_own_group_is_never_the_target() {
        // A payload exec'd into the daemon's own process group would otherwise
        // take the daemon down with it.
        let me = std::process::id();
        let mine = group_of(me);
        assert!(mine.is_some());
        let chosen = mine.filter(|&g| g > 1 && Some(g) != group_of(me));
        assert_eq!(chosen, None, "the group filter excludes our own");
    }
}

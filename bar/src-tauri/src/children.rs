//! The children that run for as long as the bar does: `pw-record` for the
//! visualiser, `pactl` for the volume.
//!
//! Both are read through a pipe, and both need the two things `std` does not
//! offer: a read that gives up, and a child that cannot outlive its parent.

use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::Command;
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

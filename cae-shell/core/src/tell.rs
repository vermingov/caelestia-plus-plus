//! Telling the person what the shell has just done.
//!
//! A mode turned on, a recording started, a setting that could not be
//! written: small things that want saying once and then forgetting. They go
//! out as notifications marked transient, which the shell's own server shows
//! and does not keep — so they behave like the old shell's toasts without
//! being a second kind of thing on screen.

use std::process::Command;

/// How loud it is, which is only how it is marked: the shell has one voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    Said,
    Warned,
}

/// Says it, through whatever is serving notifications — which is usually
/// this shell itself. Detached: nothing waits for it to be read.
pub fn tell(title: &str, body: &str, glyph: &str, how: How) {
    let urgency = match how {
        How::Said => "normal",
        How::Warned => "critical",
    };
    let _ = Command::new("setsid")
        .args(["-f", "notify-send", "-a", "caelestia", "-u", urgency, "-h", "boolean:transient:true", "-i", glyph, title, body])
        .status();
}

/// The everyday case.
pub fn said(title: &str, body: &str, glyph: &str) {
    tell(title, body, glyph, How::Said);
}

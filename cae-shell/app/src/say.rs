//! What the libraries under the shell have to say, when it is asked for.
//!
//! GPUI and wgpu report what they are doing through the `log` crate — which
//! GPU they picked and why, what a surface would not do — and with no logger
//! installed every word of it is dropped. That is fine until something only
//! happens on somebody else's machine, and then it is the difference between
//! a diagnosis and a guess: the shell said "cannot open the notification
//! corner", and the reason it could not was three lines further down, in a
//! message nobody was listening for.
//!
//! Off unless asked for, because the shell's own messages are the ones worth
//! reading in an ordinary run and a wall of Vulkan chatter would bury them.
//!
//!     CAE_LOG=info    which GPU, which formats, what was configured
//!     CAE_LOG=debug   and every frame's worth of it
//!
//! Set it in `~/.config/caelestia/cae-shell.env`, which the unit reads.

use std::io::Write;

use log::{Level, LevelFilter, Metadata, Record};

/// What counts as ours, for the level asked for. Everything else is held at
/// warnings whatever is asked, unless the asker says `all`.
const OURS: [&str; 3] = ["cae_shell", "cae_core", "gpui"];

struct ToStderr {
    ours: LevelFilter,
    theirs: LevelFilter,
}

impl log::Log for ToStderr {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let target = metadata.target();
        let wanted = if OURS.iter().any(|ours| target.starts_with(ours)) { self.ours } else { self.theirs };
        metadata.level() <= wanted
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // Straight to stderr, which is the journal when the unit runs it and
        // the terminal when a person does. One write, so two threads logging
        // at once cannot interleave halfway through a line.
        let line = format!("{:<5} {}: {}\n", record.level(), record.target(), record.args());
        let _ = std::io::stderr().write_all(line.as_bytes());
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

/// Installs it, if `CAE_LOG` asks for one.
///
/// The level asked for is ours; everything under us is held at warnings.
/// This is not tidiness. `debug` across the whole tree means naga, the
/// shader compiler inside wgpu, prints its entire type-resolution trace for
/// every shader it builds — thousands of lines at startup, which buries
/// whatever was being looked for, rate-limits the journal so that the next
/// thing is lost as well, and is slow enough to be mistaken for the shell
/// being slow. Asking a question should not change the answer.
///
/// `CAE_LOG=all=debug` lifts that for the rare case where the question is
/// actually about a library. An unknown value is taken as `info`: somebody
/// who set the variable wants to hear something.
pub fn start() {
    let Ok(asked) = std::env::var("CAE_LOG") else { return };
    let asked = asked.trim().to_ascii_lowercase();
    let (everything, asked) = match asked.strip_prefix("all=") {
        Some(rest) => (true, rest.to_string()),
        None => (false, asked),
    };
    let level = match asked.as_str() {
        "off" | "" => return,
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    };
    let theirs = if everything { level } else { level.min(LevelFilter::Warn) };
    if log::set_boxed_logger(Box::new(ToStderr { ours: level, theirs })).is_ok() {
        // The cap has to let the loudest of the two through; `enabled` does
        // the rest.
        log::set_max_level(level.max(theirs));
        log::log!(Level::Info, "cae: logging at {level}, everything else at {theirs}");
    }
}

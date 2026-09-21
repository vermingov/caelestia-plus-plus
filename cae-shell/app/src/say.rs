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

struct ToStderr;

impl log::Log for ToStderr {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
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
/// An unknown value is taken as `info` rather than ignored: somebody who set
/// the variable wants to hear something.
pub fn start() {
    let Ok(asked) = std::env::var("CAE_LOG") else { return };
    let level = match asked.trim().to_ascii_lowercase().as_str() {
        "off" | "" => return,
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    };
    if log::set_boxed_logger(Box::new(ToStderr)).is_ok() {
        log::set_max_level(level);
        log::log!(Level::Info, "cae: logging at {level}");
    }
}

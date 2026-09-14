//! Which daemon is speaking.
//!
//! Both daemons prefix every line with their own bracketed tag, and the
//! journal is read with `-u redwalld` or `-u redguardd`, so a shared module
//! that hardcoded one of them would mislabel half its output. The binary sets
//! the tag once at startup and the shared code asks for it.

use std::sync::OnceLock;

static TAG: OnceLock<String> = OnceLock::new();

/// Called once, before anything else logs. A second call is ignored rather
/// than panicking: mislabelled output is not worth aborting a root daemon.
pub fn set_tag(tag: &str) {
    let _ = TAG.set(tag.to_string());
}

pub fn tag() -> &'static str {
    TAG.get().map(String::as_str).unwrap_or("red")
}

/// Normal operation, to stdout. Rust line-buffers stdout even into a pipe, so
/// the journal sees each line as it happens without an explicit flush.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        println!("[{}] {}", $crate::log::tag(), format_args!($($arg)*))
    };
}

/// Something went wrong but the daemon carries on, to stderr.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        eprintln!("[{}] {}", $crate::log::tag(), format_args!($($arg)*))
    };
}

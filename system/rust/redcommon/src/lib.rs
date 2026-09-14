//! The parts both privileged daemons need: the JSON subset their UI protocol
//! speaks, the Unix-socket server the bar connects to, the persisted rule
//! store, the `/proc` reads that identify a process, and the log tag that says
//! which daemon is talking.
//!
//! Deliberately dependency-free. These run as root — redwall on every new
//! outbound connection, redguard on every exec — so the dependency surface is
//! worth keeping at zero.

pub mod json;
pub mod log;
pub mod procfs;
pub mod rules;
pub mod ui_sock;

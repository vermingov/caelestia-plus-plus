//! What cae knows about the desktop.
//!
//! Hyprland, the kernel's readouts, PipeWire, the tray, MPRIS, the daemons on
//! the system bus, and the notification server. None of it draws anything and
//! none of it knows what does: every module hands its results to a callback
//! on a thread of its own, and the interface decides what to do with them.
//!
//! These are the same files the Tauri bar is built from, shared by path until
//! that bar is retired, so that a fix to one is a fix to both. When it goes
//! they move here and the `#[path]` lines go with it.

pub mod about;
pub mod battery;
pub mod bluetooth;
pub mod checkup;
pub mod config;
pub mod features;
pub mod gamemode;
pub mod handover;
pub mod hyprmod;
pub mod idling;
pub mod levels;
pub mod logind;
pub mod lyrics;
pub mod machine;
pub mod monitors;
pub mod network;
pub mod recorder;
pub mod scheme;
pub mod tell;
pub mod unlocking;
pub mod session;
pub mod streams;
pub mod thumbs;
pub mod updates;
pub mod weather;
pub mod web;

#[path = "../../../bar/src-tauri/src/children.rs"]
pub mod children;
#[path = "../../../bar/src-tauri/src/gpus.rs"]
pub mod gpus;
#[path = "../../../bar/src-tauri/src/guards.rs"]
pub mod guards;
#[path = "../../../bar/src-tauri/src/hypr.rs"]
pub mod hypr;
#[path = "../../../bar/src-tauri/src/icons.rs"]
pub mod icons;
#[path = "../../../bar/src-tauri/src/launcher/mod.rs"]
pub mod launcher;
#[path = "../../../bar/src-tauri/src/logo.rs"]
pub mod logo;
#[path = "../../../bar/src-tauri/src/media.rs"]
pub mod media;
#[path = "../../../bar/src-tauri/src/notifs/mod.rs"]
pub mod notifs;
#[path = "../../../bar/src-tauri/src/services.rs"]
pub mod services;
#[path = "../../../bar/src-tauri/src/signals.rs"]
pub mod signals;
#[path = "../../../bar/src-tauri/src/spectrum.rs"]
pub mod spectrum;
#[path = "../../../bar/src-tauri/src/startup.rs"]
pub mod startup;
#[path = "../../../bar/src-tauri/src/system.rs"]
pub mod system;
#[path = "../../../bar/src-tauri/src/tray.rs"]
pub mod tray;
#[path = "../../../bar/src-tauri/src/volume.rs"]
pub mod volume;
#[path = "../../../bar/src-tauri/src/watcher.rs"]
pub mod watcher;

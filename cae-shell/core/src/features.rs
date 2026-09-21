//! The machine's modes: the few switches that change how it behaves rather
//! than how it looks.
//!
//! Maximum performance and anti-heat have a privileged half apiece, which
//! installs itself the first time the switch is turned on and stays
//! installed. Nothing here is that half: this writes the same state files
//! the shell has always written, and asks for the same installer. Keeping
//! the lid open is a `systemd-inhibit` held for as long as the switch is on.

use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};

use crate::{services, tell};

/// One switch: what it is called, what it does, and whether it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mode {
    pub id: &'static str,
    pub name: &'static str,
    pub said: &'static str,
    pub glyph: &'static str,
    pub enabled: bool,
    /// Whether its privileged half is in place. Nothing to install: true.
    pub ready: bool,
}

/// Where the shell keeps what it has been told.
fn state_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    let state = std::env::var_os("XDG_STATE_HOME").map_or_else(|| std::path::PathBuf::from(home).join(".local/state"), std::path::PathBuf::from);
    Some(state.join("caelestia"))
}

fn flag(name: &str) -> bool {
    state_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join(name)).ok())
        .is_some_and(|text| text.trim() == "1")
}

fn set_flag(name: &str, on: bool) {
    let Some(dir) = state_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(name), if on { "1\n" } else { "0\n" });
}

/// The two that share one file, since they are the shell's own business
/// rather than a daemon's.
fn modes_file() -> serde_json::Value {
    state_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join("features.json")).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn said_in_file(key: &str) -> bool {
    modes_file().get(key).and_then(serde_json::Value::as_bool).unwrap_or(false)
}

fn write_to_file(key: &str, on: bool) {
    let Some(dir) = state_dir() else { return };
    let mut modes = modes_file();
    if let Some(object) = modes.as_object_mut() {
        object.insert(key.to_string(), serde_json::Value::Bool(on));
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("features.json"), format!("{modes:#}\n"));
}

/// Whether a feature's privileged half is installed, by the unit it leaves
/// behind.
fn installed(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["is-enabled", "--quiet", unit])
        .status()
        .is_ok_and(|status| status.success())
}

/// Whether this is a machine with a lid to close.
pub fn is_laptop() -> bool {
    std::fs::read_dir("/sys/class/power_supply")
        .map(|supplies| {
            supplies.filter_map(Result::ok).any(|supply| {
                std::fs::read_to_string(supply.path().join("type")).is_ok_and(|kind| kind.trim() == "Battery")
            })
        })
        .unwrap_or(false)
}

/// Every mode there is, in the order the menu shows them.
pub fn modes() -> Vec<Mode> {
    let mut modes = vec![
        Mode {
            id: "maxPerf",
            name: "Maximum performance",
            said: "Pins the processor and the fans to their fastest",
            glyph: "bolt",
            enabled: flag("max-perf"),
            ready: installed("max-perf-sync.path"),
        },
        Mode {
            id: "antiHeat",
            name: "Anti-Heat",
            said: "Undervolts and lets the fans lead the heat",
            glyph: "ac_unit",
            enabled: flag("anti-heat"),
            ready: installed("anti-heat-sync.path"),
        },
        Mode {
            id: "dynamic",
            name: "Dynamic power",
            said: "Switches the power profile by what the machine is doing",
            glyph: "auto_mode",
            enabled: flag("dynamic"),
            ready: installed("dynamic-sync.path"),
        },
    ];
    if is_laptop() {
        modes.push(Mode {
            id: "lidStay",
            name: "Stay awake on lid close",
            said: "Closing the lid neither suspends nor idles the machine",
            glyph: "laptop_windows",
            enabled: said_in_file("lidStay"),
            ready: true,
        });
    }
    modes
}

/// Turns one on or off. Blocking: a state file, a profile over the bus, and
/// on the first turn of two of them an installer that asks for a password.
pub fn set(id: &str, on: bool) {
    match id {
        "maxPerf" => {
            set_flag("max-perf", on);
            if on {
                // Both of these drive the power profile; never both at once.
                set_flag("dynamic", false);
            }
            services::set_power_profile(if on { "performance" } else { "balanced" });
            crate::gamemode::set(on);
            if on {
                install("max-perf");
            }
        }
        "antiHeat" => {
            set_flag("anti-heat", on);
            if on {
                install("anti-heat");
            }
            tell::said(
                if on { "Anti-Heat on" } else { "Anti-Heat off" },
                if on { "Undervolted, and the fans lead the heat" } else { "Stock voltages and fan behaviour" },
                "ac_unit",
            );
        }
        "dynamic" => {
            services::set_dynamic(on);
            if on {
                install("dynamic");
            }
        }
        "lidStay" => {
            write_to_file("lidStay", on);
            hold_the_lid(on);
        }
        _ => {}
    }
}

/// Asks for the privileged half of a feature, once. The installer is the
/// shell's own script, run through `pkexec`, and it is the only thing here
/// that ever asks for a password.
fn install(feature: &str) {
    let Some(home) = std::env::var_os("HOME") else { return };
    let script = std::path::PathBuf::from(home).join(format!(".config/quickshell/caelestia/system/{feature}/install.sh"));
    if !script.is_file() {
        return eprintln!("cae: no installer for {feature} at {}", script.display());
    }
    tell::said("Setting up", "The privileged half wants your password", "key");
    let _ = Command::new("setsid")
        .args(["-f", "timeout", "600", "pkexec", "bash"])
        .arg(script)
        .status();
}

/// What holds the lid open: one `systemd-inhibit` that sleeps, for as long
/// as the switch is on.
fn holder() -> &'static Mutex<Option<Child>> {
    static HOLDER: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    HOLDER.get_or_init(|| Mutex::new(None))
}

fn hold_the_lid(on: bool) {
    let Ok(mut held) = holder().lock() else { return };
    if let Some(mut holding) = held.take() {
        let _ = holding.kill();
        let _ = holding.wait();
    }
    if !on {
        return;
    }
    let mut asking = Command::new("systemd-inhibit");
    asking.args([
        "--what=handle-lid-switch:sleep:idle",
        "--who=Caelestia",
        "--why=Stay awake on lid close",
        "sleep",
        "infinity",
    ]);
    // It must not outlive the shell. Killing it on the way out covers an
    // orderly exit; this covers the rest, because the signal comes from the
    // kernel rather than from anything the dying process still has to run.
    unsafe {
        use std::os::unix::process::CommandExt;
        asking.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let started = asking.spawn();
    *held = started.map_err(|error| eprintln!("cae: cannot hold the lid open: {error}")).ok();
}

/// Takes up again whatever was on when the shell last ran: the lid holder is
/// a process, and processes do not survive a restart.
pub fn resume() {
    if said_in_file("lidStay") {
        hold_the_lid(true);
    }
}

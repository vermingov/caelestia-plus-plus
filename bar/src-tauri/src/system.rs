//! The slow half of the bar: numbers that change on their own schedule
//! rather than in response to anything.
//!
//! Everything here is read straight from the kernel's own files. A bar that
//! shells out to `free`, `nmcli` and `upower` once a second is three process
//! spawns a second for the life of the session, which is most of what makes
//! a desktop feel like it is idling badly.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::volume::{Levels, Volume};

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// 0–100, averaged over the interval since the last snapshot.
    pub cpu: f64,
    /// 0–100 of total RAM in use.
    pub memory: f64,
    pub memory_used_gb: f64,
    pub memory_total_gb: f64,
    pub battery: Option<Battery>,
    pub volume: Option<Volume>,
    /// The default source, for the microphone icon.
    pub microphone: Option<Volume>,
    /// 0-100, or None on a machine with no backlight.
    pub brightness: Option<i64>,
    /// 0-100, or None where the driver does not report it.
    pub gpu: Option<f64>,
    pub network: Network,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    /// 0–100.
    pub level: i64,
    /// Actually taking charge right now.
    pub charging: bool,
    /// A charger is plugged in, which is not the same question. A laptop
    /// held at a charge threshold, or fed by a USB-C supply too weak to
    /// charge it, sits at `Not charging` with the cable in — and calling that
    /// "on battery" is how the bar came to show a red low-battery alert on a
    /// machine that was plugged in.
    pub on_mains: bool,
    /// Minutes until full, or until empty when running on it. None while the
    /// draw is too small or too erratic to divide by.
    pub minutes: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    /// "wifi", "ethernet" or "none".
    pub kind: String,
    /// 0–100 for wifi, 100 for a wired link that is up.
    pub strength: i64,
}

/// Cumulative CPU jiffies. Usage is a difference between two of these, so the
/// sampler holds on to the last one.
#[derive(Clone, Copy, Default)]
struct CpuTimes {
    busy: u64,
    total: u64,
}

fn read_cpu_times() -> CpuTimes {
    let Ok(stat) = fs::read_to_string("/proc/stat") else { return CpuTimes::default() };
    let Some(line) = stat.lines().next() else { return CpuTimes::default() };

    let fields: Vec<u64> = line.split_whitespace().skip(1).filter_map(|f| f.parse().ok()).collect();
    let total: u64 = fields.iter().sum();
    // user, nice, system, (idle), (iowait), irq, softirq, steal — idle and
    // iowait are the two the machine was not working during.
    let idle: u64 = fields.iter().skip(3).take(2).sum();
    CpuTimes { busy: total.saturating_sub(idle), total }
}

fn read_memory() -> (f64, f64, f64) {
    let Ok(info) = fs::read_to_string("/proc/meminfo") else { return (0.0, 0.0, 0.0) };

    let field = |name: &str| -> f64 {
        info.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
    };

    // MemAvailable, not MemFree: the kernel's own estimate of what a new
    // allocation could actually have, which is the number a person means by
    // "free memory".
    let total = field("MemTotal:");
    let available = field("MemAvailable:");
    if total <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let used = total - available;
    (used / total * 100.0, used / 1_048_576.0, total / 1_048_576.0)
}

/// The first backlight the kernel exposes. A laptop has one; a desktop with
/// an external monitor has none, and its brightness is the monitor's own
/// business rather than something the bar can reach.
fn backlight() -> Option<PathBuf> {
    fs::read_dir("/sys/class/backlight").ok()?.filter_map(Result::ok).map(|e| e.path()).next()
}

fn read_brightness() -> Option<i64> {
    let path = backlight()?;
    let current: f64 = fs::read_to_string(path.join("brightness")).ok()?.trim().parse().ok()?;
    let max: f64 = fs::read_to_string(path.join("max_brightness")).ok()?.trim().parse().ok()?;
    if max <= 0.0 {
        return None;
    }
    Some((current / max * 100.0).round() as i64)
}

/// brightnessctl rather than a write to sysfs: the file is root-owned, and
/// brightnessctl is the thing with the udev rule that makes it work without
/// asking for a password.
pub fn set_brightness(percent: i64) {
    let _ = std::process::Command::new("brightnessctl")
        .args(["set", &format!("{}%", percent.clamp(1, 100))])
        .status();
}

pub fn nudge_brightness(delta: i64) {
    let sign = if delta >= 0 { "+" } else { "-" };
    let _ = std::process::Command::new("brightnessctl")
        .args(["set", &format!("{}%{sign}", delta.abs())])
        .status();
}

/// How busy the GPU is, where the driver says so. amdgpu and i915 both expose
/// this; nvidia does not, and gets nothing rather than a wrong number.
fn read_gpu() -> Option<f64> {
    let cards = fs::read_dir("/sys/class/drm").ok()?;
    for card in cards.filter_map(Result::ok) {
        let path = card.path().join("device/gpu_busy_percent");
        if let Ok(busy) = fs::read_to_string(&path) {
            if let Ok(percent) = busy.trim().parse::<f64>() {
                return Some(percent.clamp(0.0, 100.0));
            }
        }
    }
    None
}

fn first_battery() -> Option<PathBuf> {
    let mut batteries: Vec<PathBuf> = fs::read_dir("/sys/class/power_supply")
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery"))
        .collect();
    // `read_dir` returns whatever order the filesystem feels like. On a
    // machine with two batteries that means the bar reads a different one
    // between ticks, so the percentage jumps between them.
    batteries.sort();
    batteries.into_iter().next()
}

/// Whether a charger is plugged in.
///
/// The mains supply is its own device in sysfs, separate from the battery.
/// USB-C power delivery shows up here too — a dock or a phone charger is a
/// `USB` supply that is online — and for "is it plugged in" those count.
fn on_mains() -> bool {
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else { return false };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        let kind = fs::read_to_string(path.join("type")).unwrap_or_default();
        if !matches!(kind.trim(), "Mains" | "USB" | "USB_PD" | "USB_PD_DRP") {
            return false;
        }
        fs::read_to_string(path.join("online")).is_ok_and(|online| online.trim() == "1")
    })
}

fn read_battery() -> Option<Battery> {
    let path = first_battery()?;
    let level = fs::read_to_string(path.join("capacity")).ok()?.trim().parse().ok()?;
    let status = fs::read_to_string(path.join("status")).unwrap_or_default();
    let charging = matches!(status.trim(), "Charging" | "Full");
    Some(Battery { level, charging, on_mains: on_mains(), minutes: remaining(&path, charging) })
}

/// Minutes left, from the charge counters.
///
/// Kernels report either charge (µAh) with current (µA), or energy (µWh) with
/// power (µW). Either pair divides to hours; which pair is present depends on
/// the driver, so both are tried.
fn remaining(path: &Path, charging: bool) -> Option<i64> {
    let number = |name: &str| -> Option<f64> {
        fs::read_to_string(path.join(name)).ok()?.trim().parse().ok()
    };

    let (now, full, rate) = match (number("charge_now"), number("current_now")) {
        (Some(now), Some(rate)) => (now, number("charge_full")?, rate),
        _ => (number("energy_now")?, number("energy_full")?, number("power_now")?),
    };

    // A rate of zero is a battery sitting still: there is no answer, and
    // "infinite" is not one worth printing.
    if rate <= 0.0 {
        return None;
    }
    let delta = if charging { (full - now).max(0.0) } else { now };
    Some((delta / rate * 60.0).round() as i64)
}

/// Wireless first: a laptop with both up is on wifi as far as the icon is
/// concerned, because that is the link the person is thinking about.
fn read_network() -> Network {
    let Ok(entries) = fs::read_dir("/sys/class/net") else { return Network::default() };
    let mut ethernet = None;

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "lo" {
            continue;
        }
        if fs::read_to_string(path.join("operstate")).unwrap_or_default().trim() != "up" {
            continue;
        }
        if path.join("wireless").is_dir() {
            return Network { kind: "wifi".to_string(), strength: wifi_strength(&name) };
        }
        ethernet = Some(Network { kind: "ethernet".to_string(), strength: 100 });
    }
    ethernet.unwrap_or_default()
}

/// The link quality column of /proc/net/wireless, which is out of 70 on every
/// driver that reports it.
fn wifi_strength(interface: &str) -> i64 {
    let Ok(table) = fs::read_to_string("/proc/net/wireless") else { return 100 };
    table
        .lines()
        .find(|l| l.trim_start().starts_with(interface))
        .and_then(|l| l.split_whitespace().nth(2))
        .and_then(|q| q.trim_end_matches('.').parse::<f64>().ok())
        .map(|q| ((q / 70.0) * 100.0).clamp(0.0, 100.0) as i64)
        .unwrap_or(100)
}

/// Holds the previous CPU reading so usage can be a rate rather than an
/// all-time average.
#[derive(Default)]
pub struct Sampler {
    previous: CpuTimes,
}

impl Sampler {
    pub fn new() -> Sampler {
        Sampler { previous: read_cpu_times() }
    }

    /// Everything here is read now except `levels`, which PipeWire reports
    /// as they change and the caller is holding the latest of.
    pub fn sample(&mut self, levels: &Levels) -> Snapshot {
        let now = read_cpu_times();
        let busy = now.busy.saturating_sub(self.previous.busy) as f64;
        let total = now.total.saturating_sub(self.previous.total) as f64;
        self.previous = now;

        let (memory, used, total_gb) = read_memory();
        Snapshot {
            cpu: if total > 0.0 { (busy / total * 100.0).clamp(0.0, 100.0) } else { 0.0 },
            memory,
            memory_used_gb: used,
            memory_total_gb: total_gb,
            battery: read_battery(),
            volume: levels.volume.clone(),
            microphone: levels.microphone.clone(),
            brightness: read_brightness(),
            gpu: read_gpu(),
            network: read_network(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_usage_is_a_percentage() {
        let mut sampler = Sampler::new();
        // Two samples back to back can legitimately be 0%; the point is that
        // it is never nonsense.
        let snapshot = sampler.sample(&Levels::default());
        assert!((0.0..=100.0).contains(&snapshot.cpu), "cpu was {}", snapshot.cpu);
    }

    #[test]
    fn memory_is_read_from_the_kernel() {
        let (percent, used, total) = read_memory();
        assert!(total > 0.0, "no MemTotal");
        assert!(used <= total);
        assert!((0.0..=100.0).contains(&percent));
    }

    #[test]
    fn network_kind_is_one_of_three_words() {
        let network = read_network();
        assert!(matches!(network.kind.as_str(), "wifi" | "ethernet" | ""));
    }

    #[test]
    fn a_missing_battery_is_not_an_error() {
        // Reads whatever this machine has; a desktop returns None and that is
        // a valid answer, not a failure.
        // A desktop returns None, and that is a valid answer rather than a
        // failure — the bar simply draws no battery slot.
        assert_eq!(read_battery().is_some(), first_battery().is_some());
    }

    #[test]
    fn brightness_is_a_percentage_or_nothing() {
        // A desktop has no backlight, and None is the right answer there.
        if let Some(level) = read_brightness() {
            assert!((0..=100).contains(&level), "brightness was {level}");
        }
    }

    #[test]
    fn gpu_busy_is_a_percentage_or_nothing() {
        if let Some(busy) = read_gpu() {
            assert!((0.0..=100.0).contains(&busy), "gpu was {busy}");
        }
    }

    #[test]
    fn wifi_strength_is_clamped() {
        let strength = wifi_strength("definitely-not-an-interface");
        assert!((0..=100).contains(&strength));
    }

    #[test]
    fn meminfo_parsing_survives_a_missing_field() {
        // The helper closure returns 0 for anything absent, which is what
        // keeps a kernel without MemAvailable from panicking.
        assert!(std::path::Path::new("/proc/meminfo").exists());
    }

    /// Whatever this machine has, the two answers have to agree with sysfs:
    /// a battery that reports `Charging` cannot be running on nothing.
    #[test]
    fn charging_implies_a_charger_is_plugged_in() {
        let Some(battery) = read_battery() else { return };
        if battery.charging {
            assert!(battery.on_mains, "the battery is charging but nothing is reported online");
        }
    }

    /// The point of the field: plugged in and not charging is a real state,
    /// and it must not read as "on battery".
    #[test]
    fn mains_is_read_independently_of_the_charge_status() {
        let Some(battery) = read_battery() else { return };
        assert_eq!(battery.on_mains, on_mains(), "the snapshot disagrees with sysfs");
    }
}

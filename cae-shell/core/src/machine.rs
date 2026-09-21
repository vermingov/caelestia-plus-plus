//! What the machine is made of and how hard it is working, for the page of
//! the dashboard that says so: the processor and the graphics card by name
//! and by temperature, the disks, and the traffic on the network.
//!
//! All of it is the kernel's, read from files. The bar's own readouts
//! (`system`) are what it needs every second for as long as the desktop is
//! up; these are read only while somebody is looking at them.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|text| text.trim().to_string())
}

fn number(path: impl AsRef<Path>) -> Option<f64> {
    read(path)?.parse().ok()
}

fn entries(dir: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    found.sort();
    found
}

/// What `/proc/cpuinfo` calls the processor, less what a maker adds to every
/// one of its names.
fn cpu_name_in(cpuinfo: &str) -> String {
    let named = cpuinfo.lines().find_map(|line| line.strip_prefix("model name")).and_then(|rest| rest.split_once(':'));
    let name = named.map_or("", |(_, name)| name.trim());
    let plain = name.replace("(R)", "").replace("(TM)", "").replace(" CPU", "").replace(" Processor", "");
    let plain = plain.split(" with ").next().unwrap_or_default();
    let plain = plain.split(" @").next().unwrap_or_default();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn cpu_name() -> String {
    cpu_name_in(&read("/proc/cpuinfo").unwrap_or_default())
}

/// The sensors that are the processor's own, by what their drivers are
/// called, best first.
const CPU_SENSORS: [&str; 5] = ["k10temp", "coretemp", "zenpower", "cpu_thermal", "acpitz"];

/// The processor's temperature in degrees Celsius, where a sensor says.
pub fn cpu_celsius() -> Option<f64> {
    let sensors = entries("/sys/class/hwmon");
    CPU_SENSORS.iter().find_map(|wanted| {
        let sensor = sensors.iter().find(|sensor| read(sensor.join("name")).as_deref() == Some(*wanted))?;
        number(sensor.join("temp1_input")).map(|millidegrees| millidegrees / 1000.)
    })
}

/// The graphics card the kernel lists first with a render node, which on a
/// machine with two is the one the desktop is drawn on.
fn first_card() -> Option<PathBuf> {
    entries("/sys/class/drm").into_iter().find(|card| {
        let name = card.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        name.starts_with("card") && !name.contains('-') && card.join("device/gpu_busy_percent").exists()
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gpu {
    /// 0 to 100, where the driver says.
    pub busy: Option<f64>,
    pub celsius: Option<f64>,
}

/// Mesa's drivers say in sysfs. NVIDIA's own says only to its own tool,
/// which is asked where there is no card that does.
pub fn gpu() -> Gpu {
    if let Some(card) = first_card() {
        let sensor = entries(&card.join("device/hwmon").to_string_lossy()).into_iter().next();
        return Gpu {
            busy: number(card.join("device/gpu_busy_percent")),
            celsius: sensor.and_then(|sensor| number(sensor.join("temp1_input"))).map(|millidegrees| millidegrees / 1000.),
        };
    }
    let asked = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=utilization.gpu,temperature.gpu", "--format=csv,noheader,nounits"])
        .output();
    let Ok(said) = asked else { return Gpu::default() };
    let said = String::from_utf8_lossy(&said.stdout);
    let mut fields = said.lines().next().unwrap_or_default().split(',').map(|field| field.trim().parse::<f64>().ok());
    Gpu { busy: fields.next().flatten(), celsius: fields.next().flatten() }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Disk {
    pub mount: String,
    pub device: String,
    /// In bytes.
    pub used: u64,
    pub total: u64,
}

impl Disk {
    pub fn percent(&self) -> f64 {
        if self.total == 0 { 0. } else { self.used as f64 / self.total as f64 * 100. }
    }
}

/// Filesystems that are somebody's files on a disk, rather than the kernel's
/// view of itself or a mount of something already counted.
const ON_DISK: [&str; 13] = ["ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk", "zfs", "bcachefs"];

/// `/proc/mounts` as devices and where each is mounted, once per device: a
/// btrfs disk is mounted a dozen times over, one subvolume at a time, and is
/// one disk. The shortest mount point is the one a person means by it.
fn mounted(mounts: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (Some(device), Some(mount), Some(kind)) = (fields.next(), fields.next(), fields.next()) else { continue };
        if !ON_DISK.contains(&kind) || !device.starts_with('/') {
            continue;
        }
        // The kernel writes a space in a path as \040.
        let mount = mount.replace("\\040", " ");
        match found.iter_mut().find(|(known, _)| known == device) {
            Some((_, kept)) if mount.len() < kept.len() => *kept = mount,
            Some(_) => {}
            None => found.push((device.to_string(), mount)),
        }
    }
    found
}

fn usage(mount: &str) -> Option<(u64, u64)> {
    let path = CString::new(mount).ok()?;
    let mut about = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `statvfs` fills the struct it is given and says so by
    // returning zero; it is only read after that.
    let about = unsafe {
        if libc::statvfs(path.as_ptr(), about.as_mut_ptr()) != 0 {
            return None;
        }
        about.assume_init()
    };
    let block = about.f_frsize as u64;
    let total = about.f_blocks as u64 * block;
    Some((total - about.f_bfree as u64 * block, total))
}

/// The disks, the one the system is on first.
pub fn disks() -> Vec<Disk> {
    let mut disks: Vec<Disk> = mounted(&read("/proc/mounts").unwrap_or_default())
        .into_iter()
        .filter_map(|(device, mount)| {
            let (used, total) = usage(&mount)?;
            (total > 0).then_some(Disk { mount, device, used, total })
        })
        .collect();
    disks.sort_by_key(|disk| (disk.mount != "/", disk.mount.clone()));
    disks
}

/// Interfaces that carry somebody else's traffic again, or nobody's: counted
/// with the rest they would count it twice.
fn is_physical(interface: &str) -> bool {
    const VIRTUAL: [&str; 9] = ["lo", "docker", "veth", "br-", "virbr", "tun", "tap", "wg", "tailscale"];
    !VIRTUAL.iter().any(|prefix| interface.starts_with(prefix))
}

/// Bytes received and sent since the machine started, over every interface
/// that is a real one.
fn counted(net_dev: &str) -> (u64, u64) {
    net_dev.lines().filter_map(|line| line.split_once(':')).filter(|(interface, _)| is_physical(interface.trim())).fold(
        (0, 0),
        |(received, sent), (_, counters)| {
            let counters: Vec<u64> = counters.split_whitespace().filter_map(|counter| counter.parse().ok()).collect();
            (received + counters.first().copied().unwrap_or(0), sent + counters.get(8).copied().unwrap_or(0))
        },
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Traffic {
    /// Bytes a second, over the time since the last reading.
    pub down: f64,
    pub up: f64,
    /// Bytes since the machine started.
    pub received: u64,
    pub sent: u64,
}

/// Rates are a difference between two readings, so whoever wants them keeps
/// one of these and asks it each time.
pub struct TrafficMeter {
    last: (u64, u64),
    at: Instant,
}

impl TrafficMeter {
    pub fn new() -> TrafficMeter {
        TrafficMeter { last: counted(&read("/proc/net/dev").unwrap_or_default()), at: Instant::now() }
    }

    pub fn read(&mut self) -> Traffic {
        let (received, sent) = counted(&read("/proc/net/dev").unwrap_or_default());
        let over = self.at.elapsed().as_secs_f64().max(0.001);
        let traffic = Traffic {
            // A counter that went backwards is an interface that went away.
            down: received.saturating_sub(self.last.0) as f64 / over,
            up: sent.saturating_sub(self.last.1) as f64 / over,
            received,
            sent,
        };
        (self.last, self.at) = ((received, sent), Instant::now());
        traffic
    }
}

impl Default for TrafficMeter {
    fn default() -> TrafficMeter {
        TrafficMeter::new()
    }
}

pub fn uptime() -> Duration {
    let seconds = read("/proc/uptime").and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok());
    Duration::from_secs_f64(seconds.unwrap_or_default())
}

/// A number of bytes the way a person says it: `3.2 GB`.
pub fn bytes(count: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut amount = count.max(0.);
    let mut unit = 0;
    while amount >= 1024. && unit < UNITS.len() - 1 {
        amount /= 1024.;
        unit += 1;
    }
    if unit == 0 { format!("{amount:.0} B") } else { format!("{amount:.1} {}", UNITS[unit]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_processor_is_called_what_a_person_would_call_it() {
        let cpuinfo = "processor\t: 0\nmodel name\t: AMD Ryzen 7 7735HS with Radeon Graphics\n";
        assert_eq!(cpu_name_in(cpuinfo), "AMD Ryzen 7 7735HS");
        assert_eq!(cpu_name_in("model name\t: Intel(R) Core(TM) i7-10750H CPU @ 2.60GHz\n"), "Intel Core i7-10750H");
        assert_eq!(cpu_name_in(""), "");
    }

    #[test]
    fn a_disk_mounted_many_times_is_one_disk() {
        let mounts = "proc /proc proc rw 0 0\n\
                      /dev/nvme0n1p2 /home btrfs rw,subvol=/@home 0 0\n\
                      /dev/nvme0n1p2 / btrfs rw,subvol=/@ 0 0\n\
                      /dev/nvme0n1p2 /var/log btrfs rw,subvol=/@log 0 0\n\
                      /dev/nvme0n1p1 /boot vfat rw 0 0\n\
                      /dev/sda1 /run/media/john/Portable\\040SSD ntfs3 rw 0 0\n\
                      tmpfs /tmp tmpfs rw 0 0\n";
        assert_eq!(
            mounted(mounts),
            [
                ("/dev/nvme0n1p2".to_string(), "/".to_string()),
                ("/dev/nvme0n1p1".to_string(), "/boot".to_string()),
                ("/dev/sda1".to_string(), "/run/media/john/Portable SSD".to_string()),
            ]
        );
    }

    #[test]
    fn this_machine_has_a_disk_with_something_on_it() {
        let disks = disks();
        assert_eq!(disks.first().map(|disk| disk.mount.as_str()), Some("/"), "the system's own disk comes first");
        assert!(disks[0].used > 0 && disks[0].used <= disks[0].total);
    }

    #[test]
    fn traffic_is_counted_on_real_interfaces_only() {
        let net_dev = "Inter-|   Receive                                                |  Transmit\n \
                       face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    \
                       lo: 9000 10 0 0 0 0 0 0 9000 10 0 0 0 0 0 0\n \
                       wlan0: 5000 40 0 0 0 0 0 0 700 30 0 0 0 0 0 0\n\
                       docker0: 123 1 0 0 0 0 0 0 456 1 0 0 0 0 0 0\n  \
                       enp3s0: 1000 8 0 0 0 0 0 0 300 5 0 0 0 0 0 0\n";
        assert_eq!(counted(net_dev), (6000, 1000));
    }

    #[test]
    fn bytes_are_said_the_way_a_person_would() {
        assert_eq!(bytes(900.), "900 B");
        assert_eq!(bytes(1536.), "1.5 KB");
        assert_eq!(bytes(5. * 1024. * 1024. * 1024.), "5.0 GB");
    }
}

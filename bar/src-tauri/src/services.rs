//! The parts of the bar that are somebody else's state.
//!
//! Power profiles, the firewall and protection guards, feature modes, wifi and
//! bluetooth. None of it is the bar's to own, so none of it is reimplemented
//! here: the two guards and the feature hub already live in the shell and
//! already answer over its IPC, and asking them is what keeps the bar and the
//! shell's own panels from disagreeing about what is switched on.
//!
//! This is the slow tick. Everything in `system` is a file read; everything
//! here is a question put to somebody else — the system bus for what is
//! polled, a process for what a person asks for by opening a popout — so it
//! runs every few seconds rather than every one.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Serialize;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::guards::Guards;

/// The shell's own instance name, which is what `qs -c` wants.
const SHELL: &str = "caelestia";

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub power: Power,
    pub guards: Guards,
    pub features: Vec<Feature>,
    pub bluetooth: Bluetooth,
    /// The laptop fan-curve mode, which is a state file the shell owns rather
    /// than one of the feature hub's modes.
    pub bed_mode: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Power {
    /// Whether the auto-switching daemon is driving the profile, and which one
    /// it has currently picked.
    pub dynamic: bool,
    pub dynamic_tier: String,
    /// "power-saver", "balanced" or "performance"; empty if the daemon is not
    /// there.
    pub profile: String,
    pub available: Vec<String>,
    /// Why the machine is not delivering the profile it is set to — thermal
    /// throttling, usually. Empty when it is.
    pub degraded: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    pub id: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bluetooth {
    pub powered: bool,
    pub connected: i64,
    pub discovering: bool,
}

/// One wireless network, as the popout lists them.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Wifi {
    pub ssid: String,
    pub strength: i64,
    pub active: bool,
    /// Whether joining it needs a password we do not already have.
    pub secured: bool,
    pub known: bool,
}

/// A wired device, as the popout lists them.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Ethernet {
    pub interface: String,
    pub connected: bool,
    /// The profile it is on, which is the name a person recognises.
    pub connection: String,
}

/// A bluetooth device, as the popout lists them.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub address: String,
    pub name: String,
    pub connected: bool,
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Calls one of the shell's IPC handlers and returns what it said.
///
/// `qs` exits zero whether or not the handler exists, and says so on stdout
/// instead — so a missing target has to be read out of the reply rather than
/// out of the exit status, or every caller silently believes it succeeded.
pub fn ipc(target: &str, function: &str, args: &[&str]) -> Option<String> {
    let mut argv = vec!["-c", SHELL, "ipc", "call", target, function];
    argv.extend_from_slice(args);
    let reply = output("qs", &argv)?;
    if is_missing(&reply) {
        return None;
    }
    Some(reply)
}

/// Whether a reply is `qs` reporting that there was nobody to ask.
fn is_missing(reply: &str) -> bool {
    let reply = reply.trim();
    reply.starts_with("Target not found")
        || reply.starts_with("Function not found")
        || reply.starts_with("No such")
}

// ---- power profiles ------------------------------------------------------

/// The profile daemon's name on the system bus, and its interface's. The
/// older of the two it goes by, because every version answers to it.
const PROFILES: &str = "net.hadess.PowerProfiles";
const PROFILES_PATH: &str = "/net/hadess/PowerProfiles";

/// The system bus, connected once and kept: a connection is a socket and a
/// thread of zbus's own, which is too much to make and drop on every tick.
fn system_bus() -> Option<&'static Connection> {
    static BUS: OnceLock<Option<Connection>> = OnceLock::new();
    BUS.get_or_init(|| Connection::system().ok()).as_ref()
}

fn profile_daemon() -> Option<Proxy<'static>> {
    Proxy::new(system_bus()?, PROFILES, PROFILES_PATH, "org.freedesktop.DBus.Properties").ok()
}

/// The names in the daemon's `Profiles`, which is a list of dicts.
///
/// Listed from the thriftiest up and shown from the fastest down: the order
/// `powerprofilesctl` prints, and the one the dials were laid out in.
fn profile_names(listed: &OwnedValue) -> Vec<String> {
    Vec::<HashMap<String, OwnedValue>>::try_from(listed.clone())
        .unwrap_or_default()
        .iter()
        .rev()
        .filter_map(|profile| String::try_from(profile.get("Profile")?.clone()).ok())
        .collect()
}

/// Asked over the bus rather than through `powerprofilesctl`, which is a
/// Python script and only a client of this same interface. Two of them every
/// five seconds was a tenth of a second of CPU each: the most expensive thing
/// the bar did at rest.
fn read_power() -> Power {
    let daemon: HashMap<String, OwnedValue> = profile_daemon()
        .and_then(|daemon| daemon.call("GetAll", &(PROFILES,)).ok())
        .unwrap_or_default();
    let text = |key: &str| {
        daemon.get(key).and_then(|value| String::try_from(value.clone()).ok()).unwrap_or_default()
    };

    Power {
        profile: text("ActiveProfile"),
        available: daemon.get("Profiles").map(profile_names).unwrap_or_default(),
        // The reason alone, and nothing at all when there is none.
        degraded: text("PerformanceDegraded"),
        dynamic: flag_file("dynamic").unwrap_or(false),
        dynamic_tier: state_dir()
            .and_then(|dir| std::fs::read_to_string(dir.join("dynamic-tier")).ok())
            .map(|tier| tier.trim().to_string())
            .unwrap_or_default(),
    }
}

/// Hands the profile to the auto-switching daemon, or takes it back.
///
/// The daemon watches the state file through a systemd path unit, which is
/// also how the shell drives it — there is no service to call, only a byte to
/// write. Max-perf owns the plan when it is on, so the two are never both on.
pub fn set_dynamic(on: bool) {
    let Some(dir) = state_dir() else { return };
    let _ = std::fs::write(dir.join("dynamic"), if on { "1\n" } else { "0\n" });
    if on {
        let _ = std::fs::write(dir.join("max-perf"), "0\n");
    }
}

pub fn set_power_profile(profile: &str) {
    let Some(daemon) = profile_daemon() else { return };
    let _ = daemon.call::<_, _, ()>("Set", &(PROFILES, "ActiveProfile", Value::from(profile)));
}

// ---- the guards ----------------------------------------------------------



// ---- feature modes -------------------------------------------------------

/// Parses `maxPerf: off; antiHeat: off; lidStay: on`.
fn parse_features(status: &str) -> Vec<Feature> {
    status
        .split(';')
        .filter_map(|part| {
            let (id, state) = part.split_once(':')?;
            Some(Feature { id: id.trim().to_string(), enabled: state.trim() == "on" })
        })
        .collect()
}

/// The state directory the shell keeps its modes in.
fn state_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let state = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| format!("{home}/.local/state"));
    Some(std::path::PathBuf::from(state).join("caelestia"))
}

/// Whether a one-byte state file says on.
fn flag_file(name: &str) -> Option<bool> {
    let text = std::fs::read_to_string(state_dir()?.join(name)).ok()?;
    Some(text.trim() == "1")
}

/// The feature modes, read from the files the shell writes rather than asked
/// for over its IPC.
///
/// `qs ipc call` boots a whole QML runtime for each question, which on a
/// three-second tick was the single largest thing this process did. The files
/// are the same source of truth the shell itself reloads from.
fn read_features() -> Vec<Feature> {
    // Every feature the shell has, whether or not it has been used. A state
    // file only appears once something has been toggled, so reporting just
    // the ones with files hid the button on a machine that had never toggled
    // anything — and the button is how you toggle them, so it stayed hidden.
    // Off is the right answer for a feature with nothing written down.
    let modes = json_state("features.json");
    let flag = |id: &str, file: &str| Feature {
        id: id.to_string(),
        enabled: flag_file(file).unwrap_or(false),
    };
    let mode = |id: &str| Feature {
        id: id.to_string(),
        enabled: modes
            .as_ref()
            .and_then(|json| json.get(id))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    };

    vec![
        flag("maxPerf", "max-perf"),
        flag("antiHeat", "anti-heat"),
        // lidStay and caffeine share one JSON file.
        mode("lidStay"),
        mode("caffeine"),
    ]
}

/// One of the shell's JSON state files, when it has written one.
fn json_state(name: &str) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(state_dir()?.join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

// ---- bluetooth -----------------------------------------------------------

/// Everything BlueZ has, adapters and devices alike: by object path, then by
/// interface, then by property.
type BluezObjects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

/// Whether an object's boolean property is there and set.
fn is_set(properties: &HashMap<String, OwnedValue>, name: &str) -> bool {
    properties.get(name).and_then(|value| bool::try_from(value.clone()).ok()).unwrap_or(false)
}

/// The adapter's state and how many devices are connected, from the one call
/// `bluetoothctl` makes itself — which leaves the tick starting no processes
/// at all.
fn read_bluetooth() -> Bluetooth {
    let objects: BluezObjects = system_bus()
        .and_then(|bus| Proxy::new(bus, "org.bluez", "/", "org.freedesktop.DBus.ObjectManager").ok())
        .and_then(|bluez| bluez.call("GetManagedObjects", &()).ok())
        .unwrap_or_default();

    // The lowest path is `hci0`, which is the one `bluetoothctl` calls the
    // default; a map has no first of its own.
    let adapter = objects
        .iter()
        .filter_map(|(path, interfaces)| Some((path.as_str(), interfaces.get("org.bluez.Adapter1")?)))
        .min_by_key(|(path, _)| *path)
        .map(|(_, adapter)| adapter);
    let flag = |name: &str| adapter.is_some_and(|adapter| is_set(adapter, name));

    let connected = objects
        .values()
        .filter_map(|interfaces| interfaces.get("org.bluez.Device1"))
        .filter(|device| is_set(device, "Connected"))
        .count() as i64;

    Bluetooth { powered: flag("Powered"), connected, discovering: flag("Discovering") }
}

/// Scanning is a mode the adapter is in, not a one-off: the popout shows it
/// as a switch, the way the shell's does.
pub fn set_discovering(on: bool) {
    // `scan on` blocks holding the adapter, so it is left running and killed
    // by the matching `scan off` rather than waited on.
    crate::children::detached(std::process::Command::new("setsid").args(["-f", "bluetoothctl", "scan", if on { "on" } else { "off" }]));
}

pub fn forget_device(address: &str) {
    let _ = std::process::Command::new("bluetoothctl").args(["remove", address]).status();
}

pub fn set_bluetooth(on: bool) {
    let _ = std::process::Command::new("bluetoothctl")
        .args(["power", if on { "on" } else { "off" }])
        .status();
}

pub fn devices() -> Vec<Device> {
    let connected: Vec<String> = output("bluetoothctl", &["devices", "Connected"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_string))
        .collect();

    output("bluetoothctl", &["devices", "Paired"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            // "Device AA:BB:CC:DD:EE:FF Some Headphones"
            let rest = line.strip_prefix("Device ")?;
            let (address, name) = rest.split_once(' ')?;
            Some(Device {
                connected: connected.iter().any(|c| c == address),
                address: address.to_string(),
                name: name.to_string(),
            })
        })
        .collect()
}

pub fn connect_device(address: &str, connect: bool) {
    let verb = if connect { "connect" } else { "disconnect" };
    let _ = std::process::Command::new("bluetoothctl").args([verb, address]).status();
}

// ---- wifi ----------------------------------------------------------------

/// The networks in range, best signal first, one entry per name.
///
/// `nmcli -t` is the parseable form: colon-separated, with colons inside a
/// field escaped as `\:`.
pub fn networks() -> Vec<Wifi> {
    wifi_list(&[])
}

/// The networks NetworkManager already has in hand, without waiting on the
/// radio. `networks` lets it scan first when what it has is stale, and that
/// is several seconds of an empty list: this one answers at once, and is what
/// to show while the other is on its way.
///
/// cae's network panel opens with it. The Tauri bar never learnt to.
#[cfg_attr(feature = "tauri-ui", allow(dead_code))]
pub fn networks_at_hand() -> Vec<Wifi> {
    wifi_list(&["--rescan", "no"])
}

fn wifi_list(scanning: &[&str]) -> Vec<Wifi> {
    let known: Vec<String> = output("nmcli", &["-t", "-f", "NAME", "connection", "show"])
        .unwrap_or_default()
        .lines()
        .map(unescape)
        .collect();

    let list = [&["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "device", "wifi", "list"], scanning].concat();
    let mut found = parse_wifi(&output("nmcli", &list).unwrap_or_default());
    for wifi in &mut found {
        wifi.known = known.contains(&wifi.ssid);
    }
    found
}

/// `nmcli`'s list of access points as a list of networks, the one in use
/// first and the rest by strength.
///
/// A network is usually several access points: a router on two bands, a
/// house with repeaters. They are one row here, and that row is the one in
/// use if any of them is. Keeping whichever came first lost that whenever a
/// stronger access point of the same network was listed above the one the
/// machine was actually on, which is the ordinary case, and then nothing in
/// the list was marked as connected.
fn parse_wifi(listing: &str) -> Vec<Wifi> {
    let mut found: Vec<Wifi> = Vec::new();
    for line in listing.lines() {
        let fields = split_escaped(line);
        let [in_use, ssid, signal, security] = &fields[..] else { continue };
        if ssid.is_empty() {
            continue; // a hidden network is not something to offer
        }
        let (active, strength) = (in_use.trim() == "*", signal.parse().unwrap_or(0));

        if let Some(same) = found.iter_mut().find(|w| &w.ssid == ssid) {
            // The strength that matters is the link's own, not the best one
            // in the building.
            if active {
                (same.active, same.strength) = (true, strength);
            }
            continue;
        }
        found.push(Wifi {
            active,
            strength,
            secured: !security.is_empty() && security != "--",
            known: false,
            ssid: ssid.clone(),
        });
    }
    found.sort_by(|a, b| b.active.cmp(&a.active).then(b.strength.cmp(&a.strength)));
    found
}

fn unescape(field: &str) -> String {
    field.replace("\\:", ":")
}

/// Splits an `nmcli -t` line into exactly four fields, honouring `\:`.
fn split_escaped(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut escaped = false;
    for c in line.chars() {
        match c {
            '\\' if !escaped => escaped = true,
            ':' if !escaped => fields.push(String::new()),
            _ => {
                escaped = false;
                fields.last_mut().expect("there is always a current field").push(c);
            }
        }
    }
    fields
}

/// Joins a network. An empty password means "use what is already stored",
/// which is the right thing for one that has been joined before.
pub fn join(ssid: &str, password: &str) -> Result<(), String> {
    let mut command = std::process::Command::new("nmcli");
    command.args(["device", "wifi", "connect", ssid]);
    if !password.is_empty() {
        command.args(["password", password]);
    }
    match command.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Asks NetworkManager to look again. The list is re-read a moment later by
/// the popout; a scan takes a second or two and nmcli returns before it is
/// done.
pub fn rescan() {
    let _ = std::process::Command::new("nmcli").args(["device", "wifi", "rescan"]).status();
}

/// The wired devices, which the original popout lists alongside the wireless
/// ones — a dock or a USB adapter is a thing you connect and disconnect.
pub fn ethernet() -> Vec<Ethernet> {
    output("nmcli", &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let fields = split_escaped(line);
            let [device, kind, state, connection] = &fields[..] else { return None };
            if kind != "ethernet" {
                return None;
            }
            Some(Ethernet {
                interface: device.clone(),
                connected: state == "connected",
                connection: connection.clone(),
            })
        })
        .collect()
}

pub fn set_ethernet(interface: &str, connect: bool) {
    let verb = if connect { "connect" } else { "disconnect" };
    let _ = std::process::Command::new("nmcli").args(["device", verb, interface]).status();
}

pub fn set_wifi(on: bool) {
    let _ = std::process::Command::new("nmcli")
        .args(["radio", "wifi", if on { "on" } else { "off" }])
        .status();
}

/// Bed mode, from the state file the shell's service owns. Read rather than
/// asked for: it is one byte on disk, and a subprocess to learn it would cost
/// more than the whole rest of this tick.
fn read_bed_mode() -> Option<bool> {
    flag_file("bed-mode")
}

/// Flips it through the shell, so its toast fires and its own UI keeps up. If
/// the shell is not running there is nobody to tell, and the state file is
/// the thing the root-side path unit actually watches — so that is written
/// directly as the fallback.
pub fn toggle_bed_mode() {
    if ipc("bedMode", "toggle", &[]).is_some() {
        return;
    }
    let Some(current) = read_bed_mode() else { return };
    let Some(dir) = state_dir() else { return };
    let _ = std::fs::write(dir.join("bed-mode"), if current { "0\n" } else { "1\n" });
}

pub fn read(guards: Guards) -> Snapshot {
    Snapshot {
        power: read_power(),
        guards,
        features: read_features(),
        bluetooth: read_bluetooth(),
        bed_mode: read_bed_mode(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `nmcli` printed in a flat with repeaters: the network in use is
    /// the sixth row, under a stronger access point with the same name.
    const FLAT: &str = " :H158-381_08DD:79:WPA2\n ::65:WPA2\n :TP-Link_FB8C:64:WPA1 WPA2\n ::59:\n :H158-381_08DD_5G:57:WPA2\n*:H158-381_08DD:44:WPA2\n :Lars s:32:WPA2\n";

    #[test]
    fn the_network_in_use_is_marked_even_under_a_stronger_access_point() {
        let list = parse_wifi(FLAT);

        let current: Vec<&Wifi> = list.iter().filter(|w| w.active).collect();
        assert_eq!(current.len(), 1, "exactly one network is the one in use");
        assert_eq!(current[0].ssid, "H158-381_08DD");
        assert_eq!(current[0].strength, 44, "the strength shown is the link's own");
        assert_eq!(list[0].ssid, "H158-381_08DD", "and it comes first");
    }

    #[test]
    fn access_points_of_one_network_are_one_row_and_hidden_ones_none() {
        let list = parse_wifi(FLAT);
        let names: Vec<&str> = list.iter().map(|w| w.ssid.as_str()).collect();
        assert_eq!(names, ["H158-381_08DD", "TP-Link_FB8C", "H158-381_08DD_5G", "Lars s"]);
        assert!(list.iter().all(|w| w.secured));
    }

    #[test]
    fn profiles_are_shown_fastest_first() {
        let profile = |name: &'static str| {
            HashMap::from([("Profile", Value::from(name)), ("CpuDriver", Value::from("amd_pstate"))])
        };
        let listed = Value::from(vec![profile("power-saver"), profile("balanced"), profile("performance")]);
        let listed = OwnedValue::try_from(listed).expect("no file descriptors in it");

        assert_eq!(profile_names(&listed), ["performance", "balanced", "power-saver"]);
    }

    #[test]
    fn a_profile_list_of_the_wrong_shape_is_an_empty_one() {
        let listed = OwnedValue::try_from(Value::from("balanced")).expect("a plain string");
        assert!(profile_names(&listed).is_empty());
    }

    #[test]
    fn features_are_read_as_switches() {
        let features = parse_features("maxPerf: off; antiHeat: off; lidStay: on");
        assert_eq!(features.len(), 3);
        assert_eq!(features[0].id, "maxPerf");
        assert!(!features[0].enabled);
        assert!(features[2].enabled);
    }

    #[test]
    fn a_missing_handler_is_not_an_answer() {
        // `qs` exits zero and prints this, so a caller that only checked the
        // exit status would take it for the handler's reply.
        assert!(is_missing("Target not found."));
        assert!(is_missing("Function not found"));
        assert!(!is_missing("connected; 0 pending; 119 rules"));
        assert!(!is_missing("on"));
    }

    #[test]
    fn an_nmcli_line_splits_on_unescaped_colons_only() {
        // An SSID with a colon in it is escaped by nmcli, and splitting on it
        // would shift every field after it by one.
        let fields = split_escaped(r"*:Cafe\: Wifi:72:WPA2");
        assert_eq!(fields, vec!["*", "Cafe: Wifi", "72", "WPA2"]);
    }

    #[test]
    fn an_empty_field_is_still_a_field() {
        let fields = split_escaped(":Open Network:41:");
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[3], "");
    }
}

// ---- audio devices -------------------------------------------------------

/// One sink or source, as the audio popout lists them.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioNode {
    /// PipeWire's name for it, which is what selecting it needs.
    pub name: String,
    /// What a person would call it.
    pub description: String,
    pub default: bool,
}

fn nodes(kind: &str) -> Vec<AudioNode> {
    let default = output("pactl", &[&format!("get-default-{kind}")]).unwrap_or_default();
    let short = output("pactl", &["list", "short", &format!("{kind}s")]).unwrap_or_default();

    // The short listing has the names; the long one has the descriptions, and
    // a person picks a device by the second of those.
    let long = output("pactl", &["list", &format!("{kind}s")]).unwrap_or_default();
    let mut descriptions: Vec<(String, String)> = Vec::new();
    let (mut name, mut description) = (String::new(), String::new());
    for line in long.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Name: ") {
            name = value.to_string();
        } else if let Some(value) = line.strip_prefix("Description: ") {
            description = value.to_string();
            if !name.is_empty() {
                descriptions.push((std::mem::take(&mut name), std::mem::take(&mut description)));
            }
        }
    }

    short
        .lines()
        .filter_map(|line| {
            let name = line.split('\t').nth(1)?.to_string();
            // Every sink has a monitor, which is a source in name only: it
            // is what the sink is playing, and nobody picks it to talk into.
            if name.ends_with(".monitor") {
                return None;
            }
            let description = descriptions
                .iter()
                .find(|(known, _)| known == &name)
                .map(|(_, description)| description.clone())
                .unwrap_or_else(|| name.clone());
            Some(AudioNode { default: name == default, name, description })
        })
        .collect()
}

pub fn sinks() -> Vec<AudioNode> {
    nodes("sink")
}

pub fn sources() -> Vec<AudioNode> {
    nodes("source")
}

pub fn set_default_node(kind: &str, name: &str) {
    if kind != "sink" && kind != "source" {
        return;
    }
    let _ = std::process::Command::new("pactl")
        .args([&format!("set-default-{kind}"), name])
        .status();
}

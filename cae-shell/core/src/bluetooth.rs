//! What the settings ask of Bluetooth that the bar never had to: one device
//! in full, what is nearby and not yet paired, and how the adapter shows
//! itself to others. All of it BlueZ's, asked through `bluetoothctl`.

fn ask(args: &[&str]) -> String {
    let output = std::process::Command::new("bluetoothctl").args(args).output();
    output.map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default()
}

fn tell(args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("bluetoothctl").args(args).output().map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stdout);
    Err(said.lines().last().unwrap_or("bluetoothctl refused").trim().to_string())
}

/// One device, as `bluetoothctl info` describes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub address: String,
    pub name: String,
    /// BlueZ's word for what kind of thing it is: `audio-headset`.
    pub kind: String,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub blocked: bool,
    pub wakes: bool,
    /// Per cent, from the devices that say.
    pub battery: Option<u8>,
}

fn parse_info(address: &str, listing: &str) -> Info {
    let mut info = Info { address: address.to_string(), ..Info::default() };
    for line in listing.lines() {
        let Some((key, value)) = line.trim().split_once(": ") else { continue };
        let yes = value == "yes";
        match key {
            "Alias" => info.name = value.to_string(),
            "Name" if info.name.is_empty() => info.name = value.to_string(),
            "Icon" => info.kind = value.to_string(),
            "Paired" => info.paired = yes,
            "Connected" => info.connected = yes,
            "Trusted" => info.trusted = yes,
            "Blocked" => info.blocked = yes,
            "WakeAllowed" => info.wakes = yes,
            // "0x50 (80)": the number a person reads is the one in brackets.
            "Battery Percentage" => {
                info.battery = value.split(['(', ')']).nth(1).and_then(|percent| percent.trim().parse().ok());
            }
            _ => {}
        }
    }
    info
}

pub fn info(address: &str) -> Info {
    parse_info(address, &ask(&["info", address]))
}

/// "Device AA:BB:CC:DD:EE:FF Some Headphones", one a line.
fn parse_devices(listing: &str) -> Vec<(String, String)> {
    let devices = listing.lines().filter_map(|line| {
        let (address, name) = line.strip_prefix("Device ")?.split_once(' ')?;
        Some((address.to_string(), name.to_string()))
    });
    devices.collect()
}

/// What the adapter has heard from and is not paired with. A device that
/// has not said its name is listed by its address with dashes in it, which
/// tells nobody anything: those are left out until they do.
pub fn nearby() -> Vec<(String, String)> {
    let paired = parse_devices(&ask(&["devices", "Paired"]));
    let mut found: Vec<_> = parse_devices(&ask(&["devices"]))
        .into_iter()
        .filter(|(address, name)| !paired.iter().any(|(known, _)| known == address) && *name != address.replace(':', "-"))
        .collect();
    found.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    found
}

/// Pairs, and then does what pairing is for: trusts it, so that it may
/// connect by itself from now on, and connects it.
pub fn pair(address: &str) -> Result<(), String> {
    tell(&["pair", address])?;
    let _ = tell(&["trust", address]);
    tell(&["connect", address])
}

pub fn set_trusted(address: &str, trusted: bool) -> Result<(), String> {
    tell(&[if trusted { "trust" } else { "untrust" }, address])
}

pub fn set_blocked(address: &str, blocked: bool) -> Result<(), String> {
    tell(&[if blocked { "block" } else { "unblock" }, address])
}

pub fn set_wakes(address: &str, wakes: bool) -> Result<(), String> {
    tell(&["wake", address, if wakes { "on" } else { "off" }])
}

/// How the adapter shows itself to other devices.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Adapter {
    pub discoverable: bool,
    pub pairable: bool,
}

fn parse_adapter(listing: &str) -> Adapter {
    let flag = |name: &str| listing.lines().any(|line| line.trim() == format!("{name}: yes"));
    Adapter { discoverable: flag("Discoverable"), pairable: flag("Pairable") }
}

pub fn adapter() -> Adapter {
    parse_adapter(&ask(&["show"]))
}

pub fn set_discoverable(on: bool) -> Result<(), String> {
    tell(&["discoverable", if on { "on" } else { "off" }])
}

pub fn set_pairable(on: bool) -> Result<(), String> {
    tell(&["pairable", if on { "on" } else { "off" }])
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADSET: &str = "Device 14:3F:A6:00:11:22 (public)\n\tName: WH-1000XM4\n\tAlias: Headphones\n\tClass: 0x00240404 (2360324)\n\
        \tIcon: audio-headset\n\tPaired: yes\n\tBonded: yes\n\tTrusted: yes\n\tBlocked: no\n\tConnected: yes\n\tWakeAllowed: no\n\
        \tLegacyPairing: no\n\tBattery Percentage: 0x50 (80)\n";

    #[test]
    fn a_device_is_read_for_what_the_settings_show_of_it() {
        let info = parse_info("14:3F:A6:00:11:22", HEADSET);
        assert_eq!(info.name, "Headphones", "the alias is what the person called it");
        assert_eq!(info.kind, "audio-headset");
        assert!(info.paired && info.connected && info.trusted);
        assert!(!info.blocked && !info.wakes);
        assert_eq!(info.battery, Some(80), "the number in brackets, not the hexadecimal before it");
    }

    #[test]
    fn a_device_that_says_nothing_of_a_battery_has_none() {
        assert_eq!(parse_info("AA", "Device AA\n\tName: Mouse\n\tPaired: yes\n").battery, None);
        assert_eq!(parse_info("AA", "Device AA not available\n"), Info { address: "AA".into(), ..Info::default() });
    }

    #[test]
    fn devices_are_listed_by_address_and_name() {
        let listing = "Device 14:3F:A6:00:11:22 WH-1000XM4\nDevice 7C:96:D2:AA:BB:CC Keychron K2\n[CHG] noise\n";
        assert_eq!(
            parse_devices(listing),
            [("14:3F:A6:00:11:22".to_string(), "WH-1000XM4".to_string()), ("7C:96:D2:AA:BB:CC".to_string(), "Keychron K2".to_string())]
        );
    }

    #[test]
    fn the_adapter_says_how_it_shows_itself() {
        let adapter = parse_adapter("Controller 00:11 (public)\n\tPowered: yes\n\tDiscoverable: no\n\tPairable: yes\n");
        assert_eq!(adapter, Adapter { discoverable: false, pairable: true });
    }
}

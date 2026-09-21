//! What the settings ask of the network that the bar never had to: what a
//! wired link was given, how a profile gets its address, and forgetting a
//! network. All of it is NetworkManager's, asked through `nmcli`.

use std::net::Ipv4Addr;

fn nmcli(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("nmcli").args(args).output().map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

/// Whether the wireless radio is on, which is not the same as being
/// connected to anything.
pub fn wifi_enabled() -> bool {
    nmcli(&["-t", "-f", "WIFI", "general", "status"]).is_ok_and(|said| said.trim() == "enabled")
}

/// What a device that is up has been given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Link {
    pub address: String,
    pub gateway: String,
    pub dns: Vec<String>,
    pub mac: String,
    /// What the two ends agreed on: "1000 Mb/s". Empty for a link that is
    /// down, and for a device that has no such thing to report.
    pub speed: String,
    /// Bytes both ways since the machine started, as a person would say it.
    pub carried: String,
}

/// `nmcli -t device show`: `IP4.ADDRESS[1]:192.168.1.20/24`, one a line.
fn parse_link(listing: &str) -> Link {
    let mut link = Link::default();
    for line in listing.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        // `-t` escapes the colons in a hardware address, which are the only
        // ones a value here ever has.
        let value = value.trim().replace("\\:", ":");
        if value.is_empty() || value == "--" {
            continue;
        }
        match key {
            "GENERAL.HWADDR" => link.mac = value,
            "IP4.GATEWAY" => link.gateway = value,
            key if key.starts_with("IP4.ADDRESS") && link.address.is_empty() => {
                link.address = value.split('/').next().unwrap_or_default().to_string();
            }
            key if key.starts_with("IP4.DNS") => link.dns.push(value),
            _ => {}
        }
    }
    link
}

fn bytes(count: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut amount = count as f64;
    let mut unit = 0;
    while amount >= 1024. && unit < UNITS.len() - 1 {
        amount /= 1024.;
        unit += 1;
    }
    if unit == 0 { format!("{count} B") } else { format!("{amount:.1} {}", UNITS[unit]) }
}

pub fn link(interface: &str) -> Link {
    let fields = "GENERAL.HWADDR,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS";
    let mut link = parse_link(&nmcli(&["-t", "-f", fields, "device", "show", interface]).unwrap_or_default());

    let counter = |name: &str| -> Option<u64> {
        std::fs::read_to_string(format!("/sys/class/net/{interface}/{name}")).ok()?.trim().parse().ok()
    };
    // The kernel says -1 for a link that is down, which does not parse as a
    // speed and is not one.
    link.speed = counter("speed").map(|speed| format!("{speed} Mb/s")).unwrap_or_default();
    link.carried = match (counter("statistics/rx_bytes"), counter("statistics/tx_bytes")) {
        (Some(received), Some(sent)) => bytes(received + sent),
        _ => String::new(),
    };
    link
}

/// How a profile comes by its address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Addressing {
    /// Everything from the network.
    #[default]
    Automatic,
    /// The address from the network, the name servers from here.
    AutomaticWithDns,
    Manual,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ipv4 {
    pub addressing: Addressing,
    /// With its prefix, as it is typed: `192.168.1.50/24`.
    pub address: String,
    pub gateway: String,
    /// Separated by commas, as it is typed.
    pub dns: String,
}

fn parse_ipv4(listing: &str) -> Ipv4 {
    let mut ipv4 = Ipv4::default();
    let (mut method, mut own_dns) = (String::new(), false);
    for line in listing.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        if key == "ipv4.ignore-auto-dns" {
            own_dns = value == "yes";
        }
        if value.is_empty() || value == "--" {
            continue;
        }
        match key {
            "ipv4.method" => method = value.to_string(),
            "ipv4.addresses" => ipv4.address = value.split(',').next().unwrap_or_default().trim().to_string(),
            "ipv4.gateway" => ipv4.gateway = value.to_string(),
            "ipv4.dns" => {
                let servers: Vec<&str> = value.split([',', ';']).map(str::trim).filter(|server| !server.is_empty()).collect();
                ipv4.dns = servers.join(", ");
            }
            _ => {}
        }
    }
    ipv4.addressing = match (method.as_str(), own_dns) {
        ("manual", _) => Addressing::Manual,
        (_, true) => Addressing::AutomaticWithDns,
        _ => Addressing::Automatic,
    };
    ipv4
}

pub fn ipv4(connection: &str) -> Option<Ipv4> {
    let fields = "ipv4.method,ipv4.addresses,ipv4.gateway,ipv4.dns,ipv4.ignore-auto-dns";
    nmcli(&["-t", "-f", fields, "connection", "show", connection]).ok().map(|listing| parse_ipv4(&listing))
}

/// What the profile is set to for `ipv4`, as `nmcli connection modify` takes
/// it. Whatever the chosen way of addressing does not use is cleared rather
/// than left: a gateway kept from a manual setup would still be obeyed.
fn ipv4_settings(ipv4: &Ipv4) -> Vec<(&'static str, String)> {
    let dns = ipv4.dns.split(',').map(str::trim).filter(|server| !server.is_empty()).collect::<Vec<_>>().join(" ");
    let (method, address, gateway, dns, own_dns) = match ipv4.addressing {
        Addressing::Manual => ("manual", ipv4.address.trim().to_string(), ipv4.gateway.trim().to_string(), dns, "yes"),
        Addressing::AutomaticWithDns => ("auto", String::new(), String::new(), dns, "yes"),
        Addressing::Automatic => ("auto", String::new(), String::new(), String::new(), "no"),
    };
    vec![
        ("ipv4.method", method.to_string()),
        ("ipv4.addresses", address),
        ("ipv4.gateway", gateway),
        ("ipv4.dns", dns),
        ("ipv4.ignore-auto-dns", own_dns.to_string()),
    ]
}

/// Writes the profile and brings it up again, which is when it applies.
pub fn set_ipv4(connection: &str, ipv4: &Ipv4) -> Result<(), String> {
    let settings = ipv4_settings(ipv4);
    let mut args = vec!["connection", "modify", connection];
    for (key, value) in &settings {
        args.extend([*key, value.as_str()]);
    }
    nmcli(&args)?;
    nmcli(&["connection", "up", connection]).map(drop)
}

/// Forgets a network that was joined: the profile and the password with it.
pub fn forget(ssid: &str) -> Result<(), String> {
    nmcli(&["connection", "delete", "id", ssid]).map(drop)
}

/// An address with its prefix: `192.168.1.50/24`.
pub fn is_address_with_prefix(text: &str) -> bool {
    let Some((address, prefix)) = text.trim().split_once('/') else { return false };
    address.parse::<Ipv4Addr>().is_ok() && prefix.parse::<u8>().is_ok_and(|prefix| prefix <= 32)
}

pub fn is_address(text: &str) -> bool {
    text.trim().parse::<Ipv4Addr>().is_ok()
}

/// One or more addresses with commas between them.
pub fn is_address_list(text: &str) -> bool {
    !text.trim().is_empty() && text.split(',').all(is_address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_link_was_given_is_read_from_the_listing() {
        let listing = "GENERAL.HWADDR:74\\:56\\:3C\\:AA\\:BB\\:CC\nIP4.ADDRESS[1]:192.168.1.20/24\nIP4.ADDRESS[2]:10.0.0.4/8\n\
                       IP4.GATEWAY:192.168.1.1\nIP4.DNS[1]:1.1.1.1\nIP4.DNS[2]:9.9.9.9\n";
        let link = parse_link(listing);
        assert_eq!(link.mac, "74:56:3C:AA:BB:CC");
        assert_eq!(link.address, "192.168.1.20", "the first address is the link's");
        assert_eq!(link.gateway, "192.168.1.1");
        assert_eq!(link.dns, ["1.1.1.1", "9.9.9.9"]);

        // A device that is down lists dashes, which are not values.
        assert_eq!(parse_link("GENERAL.HWADDR:--\nIP4.GATEWAY:--\n"), Link::default());
    }

    #[test]
    fn the_three_ways_of_addressing_are_told_apart() {
        let plain = parse_ipv4("ipv4.method:auto\nipv4.addresses:\nipv4.gateway:--\nipv4.dns:\nipv4.ignore-auto-dns:no\n");
        assert_eq!(plain, Ipv4::default());

        let own_dns = parse_ipv4("ipv4.method:auto\nipv4.dns:1.1.1.1,9.9.9.9\nipv4.ignore-auto-dns:yes\n");
        assert_eq!(own_dns.addressing, Addressing::AutomaticWithDns);
        assert_eq!(own_dns.dns, "1.1.1.1, 9.9.9.9");

        let manual = parse_ipv4("ipv4.method:manual\nipv4.addresses:192.168.1.50/24\nipv4.gateway:192.168.1.1\nipv4.ignore-auto-dns:yes\n");
        assert_eq!((manual.addressing, manual.address.as_str(), manual.gateway.as_str()), (Addressing::Manual, "192.168.1.50/24", "192.168.1.1"));
    }

    #[test]
    fn going_back_to_automatic_clears_what_manual_left_behind() {
        let settings = ipv4_settings(&Ipv4 {
            addressing: Addressing::Automatic,
            address: "192.168.1.50/24".into(),
            gateway: "192.168.1.1".into(),
            dns: "1.1.1.1".into(),
        });
        assert!(settings.contains(&("ipv4.method", "auto".to_string())));
        assert!(settings.contains(&("ipv4.gateway", String::new())), "the old gateway would still be obeyed");
        assert!(settings.contains(&("ipv4.dns", String::new())));
        assert!(settings.contains(&("ipv4.ignore-auto-dns", "no".to_string())));

        let manual = ipv4_settings(&Ipv4 {
            addressing: Addressing::Manual,
            address: " 10.0.0.2/8 ".into(),
            gateway: "10.0.0.1".into(),
            dns: "1.1.1.1, 9.9.9.9".into(),
        });
        assert!(manual.contains(&("ipv4.addresses", "10.0.0.2/8".to_string())));
        assert!(manual.contains(&("ipv4.dns", "1.1.1.1 9.9.9.9".to_string())), "nmcli takes them with spaces between");
    }

    #[test]
    fn what_is_typed_is_checked_before_it_is_sent() {
        assert!(is_address_with_prefix("192.168.1.50/24"));
        assert!(!is_address_with_prefix("192.168.1.50"));
        assert!(!is_address_with_prefix("192.168.1.50/33"));
        assert!(!is_address_with_prefix("192.168.1.500/24"));
        assert!(is_address("10.0.0.1") && !is_address("10.0.0"));
        assert!(is_address_list("1.1.1.1, 9.9.9.9") && !is_address_list("1.1.1.1,") && !is_address_list(""));
    }

    #[test]
    fn bytes_are_said_the_way_a_person_would() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}

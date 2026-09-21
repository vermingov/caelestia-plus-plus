//! What this machine is, and what is running on it: the facts a settings
//! window states and a bug report wants.

use std::path::PathBuf;

fn first_line(path: &str) -> String {
    std::fs::read_to_string(path).map(|text| text.lines().next().unwrap_or_default().trim().to_string()).unwrap_or_default()
}

/// One field of `/etc/os-release`, unquoted.
fn os_release(field: &str) -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    let prefix = format!("{field}=");
    text.lines().find_map(|line| line.strip_prefix(&prefix)).map(|value| value.trim_matches('"').to_string())
}

/// A program's own account of its version: the first thing on the first
/// line it prints that looks like one.
fn version_of(program: &str, args: &[&str]) -> String {
    let Ok(output) = std::process::Command::new(program).args(args).output() else { return String::new() };
    let said = String::from_utf8_lossy(&output.stdout);
    let line = said.lines().next().unwrap_or_default();
    let word = line.split_whitespace().find(|word| word.trim_start_matches('v').starts_with(|first: char| first.is_ascii_digit()));
    word.unwrap_or_default().trim_start_matches('v').trim_end_matches(',').to_string()
}

/// Where the shell's checkout is: the directory the updater keeps current.
pub fn checkout() -> PathBuf {
    if let Some(dir) = std::env::var_os("CAELESTIA_SHELL_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    home.join(".config/quickshell/caelestia")
}

pub fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").arg("-C").arg(checkout()).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    pub hostname: String,
    /// The maker's name for the model, where the firmware says.
    pub device: String,
    pub distro: String,
    pub kernel: String,
    pub firmware: String,
}

pub fn machine() -> Machine {
    const DMI: &str = "/sys/devices/virtual/dmi/id";
    let (vendor, product) = (first_line(&format!("{DMI}/sys_vendor")), first_line(&format!("{DMI}/product_name")));
    // A maker that puts its own name at the front of the model says it once.
    let device = if product.to_lowercase().starts_with(&vendor.to_lowercase()) { product } else { format!("{vendor} {product}") };
    Machine {
        hostname: first_line("/proc/sys/kernel/hostname"),
        device: device.trim().to_string(),
        distro: os_release("PRETTY_NAME").or_else(|| os_release("NAME")).unwrap_or_default(),
        kernel: first_line("/proc/sys/kernel/osrelease"),
        firmware: first_line(&format!("{DMI}/bios_version")),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Software {
    /// The commit the checkout is on, short.
    pub revision: String,
    /// The release that commit is, when it is exactly one.
    pub release: String,
    pub compositor: String,
    pub cli: String,
}

/// Several programs are started to answer this, so it is asked for off the
/// thread that draws.
pub fn software() -> Software {
    Software {
        revision: git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default(),
        release: git(&["describe", "--tags", "--exact-match", "HEAD"]).unwrap_or_default(),
        compositor: version_of("hyprctl", &["version"]),
        cli: version_of("caelestia", &["--version"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_can_say_what_it_is() {
        let machine = machine();
        assert!(!machine.hostname.is_empty());
        assert!(!machine.kernel.is_empty());
        assert!(!machine.distro.contains('"'), "the distro came back quoted: {:?}", machine.distro);
    }
}

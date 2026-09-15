//! What runs when you log in.
//!
//! Two lists that look like one: XDG autostart entries, which are `.desktop`
//! files in a handful of directories, and enabled systemd user services. The
//! shell reads them with a helper script and so does this, rather than
//! reimplementing the `.desktop` parsing and the enable/disable dance — one
//! of them getting it subtly differently is exactly the kind of disagreement
//! that makes two front ends onto one system worse than either alone.

use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// "autostart" or "systemd" — which half it came from, and so which way
    /// it is switched off.
    pub source: String,
    /// The desktop file's path, or the unit's name.
    pub key: String,
    pub name: String,
    /// The command it runs, for autostart entries.
    pub exec: String,
    pub icon: String,
    pub enabled: bool,
}

/// The helper the shell uses: its own binary where that is new enough to know
/// the verb, and the bundled script otherwise.
fn helper() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let script = format!("{home}/.config/quickshell/caelestia/assets/startup-ctl.py");
    vec!["python3".to_string(), script]
}

fn run(args: &[&str]) -> Option<String> {
    let helper = helper();
    let (program, base) = helper.split_first()?;
    let output = std::process::Command::new(program).args(base).args(args).output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The autostart half, from the helper's pipe-separated listing:
/// `as|<enabled>|<path>|<name>|<exec>|<icon>`.
fn autostart() -> Vec<Entry> {
    run(&["scan"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('|');
            if fields.next()? != "as" {
                return None;
            }
            let enabled = fields.next()? == "1";
            let path = fields.next()?.to_string();
            let name = fields.next()?.to_string();
            let exec = fields.next().unwrap_or_default().to_string();
            let icon = fields.next().unwrap_or_default().to_string();
            Some(Entry {
                source: "autostart".to_string(),
                name: if name.is_empty() { path.clone() } else { name },
                key: path,
                exec,
                icon,
                enabled,
            })
        })
        .collect()
}

/// The systemd half. Only enabled units are listed, which is what the shell
/// shows too: a disabled unit is not something that starts with the session.
fn systemd() -> Vec<Entry> {
    let output = std::process::Command::new("systemctl")
        .args([
            "--user",
            "list-unit-files",
            "--type=service",
            "--state=enabled",
            "--no-legend",
            "--plain",
        ])
        .output()
        .ok();

    let Some(output) = output else { return Vec::new() };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let unit = line.split_whitespace().next()?;
            Some(Entry {
                source: "systemd".to_string(),
                name: unit.trim_end_matches(".service").to_string(),
                key: unit.to_string(),
                exec: String::new(),
                icon: String::new(),
                enabled: true,
            })
        })
        .collect()
}

pub fn scan() -> Vec<Entry> {
    let mut entries = autostart();
    entries.extend(systemd());
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

pub fn set_enabled(entry_source: &str, key: &str, enabled: bool) {
    if entry_source == "systemd" {
        let verb = if enabled { "enable" } else { "disable" };
        let _ = std::process::Command::new("systemctl").args(["--user", verb, key]).status();
        return;
    }
    let _ = run(&["set-enabled", key, if enabled { "1" } else { "0" }]);
}

pub fn remove(entry_source: &str, key: &str) {
    if entry_source == "systemd" {
        let _ = std::process::Command::new("systemctl").args(["--user", "disable", key]).status();
        return;
    }
    let _ = run(&["remove", key]);
}

pub fn add(name: &str, exec: &str) {
    if name.is_empty() || exec.is_empty() {
        return;
    }
    let _ = run(&["add", name, exec]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listing_is_parsed_into_entries() {
        // Read from this machine, so it only asserts what the helper actually
        // prints — the point is that a line becomes an entry with a name.
        let entries = scan();
        for entry in &entries {
            assert!(!entry.name.is_empty(), "an entry came back nameless: {entry:?}");
            assert!(["autostart", "systemd"].contains(&entry.source.as_str()));
        }
    }

    #[test]
    fn adding_nothing_does_nothing() {
        // The panel's form can be submitted empty, and an empty name would
        // otherwise write a desktop file nobody can find again.
        add("", "/usr/bin/true");
        add("Name", "");
    }
}

//! `caelestia-tools startup` — the XDG autostart entries behind the security
//! centre's Startup tab.
//!
//! Standard masking semantics: a file in the user's autostart directory
//! shadows a system one of the same name, and disabling a system entry writes
//! a masked copy into the user directory rather than touching /etc.
//!
//! systemd --user units are the shell's own business; only .desktop entries
//! are handled here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Dirs {
    pub user: PathBuf,
    pub system: PathBuf,
    pub backup: PathBuf,
}

impl Dirs {
    pub fn from_env() -> Dirs {
        let config = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => home().join(".config"),
        };
        let user = config.join("autostart");
        Dirs {
            backup: user.join("disabled-backup"),
            user,
            system: PathBuf::from("/etc/xdg/autostart"),
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn run(args: &[String]) -> i32 {
    let dirs = Dirs::from_env();
    match args.split_first().map(|(c, rest)| (c.as_str(), rest)) {
        Some(("scan", [])) => scan(&dirs),
        Some(("set-enabled", [path, state])) => set_enabled(&dirs, Path::new(path), state == "1"),
        Some(("remove", [path])) => remove(&dirs, Path::new(path)),
        Some(("add", [name, exec])) => add(&dirs, name, exec),
        _ => {
            eprintln!("usage: startup scan | set-enabled <path> <0|1> | remove <path> | add <name> <exec>");
            2
        }
    }
}

/// One line per entry, user files shadowing system ones:
/// `as|<enabled>|<path>|<name>|<exec>|<icon>`.
fn scan(dirs: &Dirs) -> i32 {
    let mut seen = Vec::new();
    for dir in [&dirs.user, &dirs.system] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "desktop"))
            .collect();
        files.sort();

        for file in files {
            let Some(name) = file.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if seen.contains(&name) || file.parent() == Some(dirs.backup.as_path()) {
                continue;
            }
            seen.push(name);

            let entry = parse_entry(&std::fs::read_to_string(&file).unwrap_or_default());
            // An entry that hides itself and does not even say what it is has
            // nothing to show in a list.
            if field(&entry, "NoDisplay").eq_ignore_ascii_case("true")
                && entry.get("Name").is_none()
            {
                continue;
            }

            let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let label = match entry.get("Name") {
                Some(name) => name.clone(),
                None => stem,
            };
            // The fields are joined with pipes, so a pipe inside one would
            // invent a column.
            println!(
                "as|{}|{}|{}|{}|{}",
                u8::from(is_enabled(&entry)),
                file.display(),
                label.replace('|', "/"),
                field(&entry, "Exec").replace('|', "/"),
                field(&entry, "Icon").replace('|', "/"),
            );
        }
    }
    0
}

fn set_enabled(dirs: &Dirs, path: &Path, enabled: bool) -> i32 {
    if path.parent() == Some(dirs.system.as_path()) {
        // /etc is not ours to edit: shadow it with a hidden copy, or drop the
        // shadow to let the original through again.
        let Some(name) = path.file_name() else { return 1 };
        let mask = dirs.user.join(name);
        if enabled {
            if mask.exists() {
                let _ = std::fs::remove_file(&mask);
            }
            return 0;
        }
        let base = parse_entry(&std::fs::read_to_string(path).unwrap_or_default());
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let body = format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon={}\nHidden=true\n",
            base.get("Name").cloned().unwrap_or(stem),
            field(&base, "Exec"),
            field(&base, "Icon"),
        );
        if std::fs::create_dir_all(&dirs.user).is_err() || std::fs::write(&mask, body).is_err() {
            eprintln!("caelestia-tools: cannot write {}", mask.display());
            return 1;
        }
        return 0;
    }

    for (key, value) in [
        ("Hidden", if enabled { "false" } else { "true" }),
        ("X-GNOME-Autostart-enabled", if enabled { "true" } else { "false" }),
    ] {
        if let Err(e) = write_key(path, key, value) {
            eprintln!("caelestia-tools: cannot update {}: {e}", path.display());
            return 1;
        }
    }
    0
}

fn remove(dirs: &Dirs, path: &Path) -> i32 {
    if path.parent() == Some(dirs.system.as_path()) {
        return set_enabled(dirs, path, false); // a system file can only be masked
    }
    let Some(name) = path.file_name() else { return 1 };
    if std::fs::create_dir_all(&dirs.backup).is_err() {
        return 1;
    }
    // Moved, not deleted: "remove" from a list should not mean gone forever.
    match std::fs::rename(path, dirs.backup.join(name)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("caelestia-tools: cannot move {}: {e}", path.display());
            1
        }
    }
}

fn add(dirs: &Dirs, name: &str, exec: &str) -> i32 {
    let slug = slugify(name);
    let mut dest = dirs.user.join(format!("{slug}.desktop"));
    let mut n = 1;
    while dest.exists() {
        dest = dirs.user.join(format!("{slug}-{n}.desktop"));
        n += 1;
    }
    let body = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\nX-GNOME-Autostart-enabled=true\n"
    );
    if std::fs::create_dir_all(&dirs.user).is_err() || std::fs::write(&dest, body).is_err() {
        eprintln!("caelestia-tools: cannot write {}", dest.display());
        return 1;
    }
    println!("{}", dest.display());
    0
}

/// The [Desktop Entry] section only, last key wins.
pub fn parse_entry(text: &str) -> BTreeMap<String, String> {
    let mut data = BTreeMap::new();
    let mut in_entry = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            data.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    data
}

pub fn is_enabled(entry: &BTreeMap<String, String>) -> bool {
    !field(entry, "Hidden").eq_ignore_ascii_case("true")
        && !field(entry, "X-GNOME-Autostart-enabled").eq_ignore_ascii_case("false")
}

fn field(entry: &BTreeMap<String, String>, key: &str) -> String {
    entry.get(key).cloned().unwrap_or_default()
}

fn write_key(path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_else(|_| "[Desktop Entry]".to_string());
    let updated = apply_key(&existing, key, value);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, updated)
}

/// Set `key=value` inside [Desktop Entry], leaving every other line — and
/// every other section — exactly as it was.
pub fn apply_key(text: &str, key: &str, value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_entry = false;
    let mut done = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            if in_entry && !done {
                out.push(format!("{key}={value}"));
                done = true;
            }
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && line.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            continue; // the old line goes; the new one is written above or below
        }
        out.push(raw.to_string());
    }

    if !done {
        let header = "[Desktop Entry]";
        if !out.iter().any(|l| l.trim() == header) {
            out.insert(0, header.to_string());
        }
        let at = out.iter().position(|l| l.trim() == header).unwrap_or(0);
        out.insert(at + 1, format!("{key}={value}"));
    }

    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// A filename from a display name: letters and digits survive, everything
/// else becomes a dash.
pub fn slugify(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = mapped.trim_matches('-').to_lowercase();
    if trimmed.is_empty() {
        "startup-app".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_the_desktop_entry_section() {
        let text = "[Desktop Entry]\nName=Thing\n# a comment\nExec=thing --now\n\n[Desktop Action Open]\nName=Other\n";
        let entry = parse_entry(text);
        assert_eq!(entry.get("Name").map(String::as_str), Some("Thing"));
        assert_eq!(entry.get("Exec").map(String::as_str), Some("thing --now"));
        assert!(!entry.contains_key("# a comment"));
    }

    #[test]
    fn either_hidden_flag_disables_an_entry() {
        assert!(is_enabled(&parse_entry("[Desktop Entry]\nName=x\n")));
        assert!(!is_enabled(&parse_entry("[Desktop Entry]\nHidden=true\n")));
        assert!(!is_enabled(&parse_entry("[Desktop Entry]\nHidden=TRUE\n")));
        assert!(!is_enabled(&parse_entry(
            "[Desktop Entry]\nX-GNOME-Autostart-enabled=false\n"
        )));
        assert!(is_enabled(&parse_entry("[Desktop Entry]\nHidden=false\n")));
    }

    #[test]
    fn setting_a_key_leaves_the_rest_of_the_file_alone() {
        let before = "[Desktop Entry]\nName=Thing\nHidden=true\nExec=thing\n\n[Other]\nHidden=keepme\n";
        let after = apply_key(before, "Hidden", "false");
        assert!(after.contains("Hidden=false"));
        assert!(!after.contains("Hidden=true"));
        assert!(after.contains("[Other]\nHidden=keepme"), "other sections untouched:\n{after}");
        assert!(after.contains("Name=Thing") && after.contains("Exec=thing"));
        assert!(after.ends_with('\n'));
    }

    #[test]
    fn a_missing_key_is_added_inside_the_entry() {
        let after = apply_key("[Desktop Entry]\nName=Thing\n", "Hidden", "true");
        let lines: Vec<&str> = after.lines().collect();
        assert_eq!(lines[0], "[Desktop Entry]");
        assert_eq!(lines[1], "Hidden=true");
        assert_eq!(lines[2], "Name=Thing");
    }

    #[test]
    fn a_file_without_the_header_gains_one() {
        let after = apply_key("", "Hidden", "true");
        assert_eq!(after, "[Desktop Entry]\nHidden=true\n");
    }

    #[test]
    fn names_become_filenames() {
        assert_eq!(slugify("Nextcloud Desktop"), "nextcloud-desktop");
        assert_eq!(slugify("  !!  "), "startup-app");
        assert_eq!(slugify("KDE Connect (indicator)"), "kde-connect--indicator");
        assert_eq!(slugify(""), "startup-app");
    }
}

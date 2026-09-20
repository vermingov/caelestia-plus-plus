//! The application list, read from the desktop entries on this machine.
//!
//! Everything here is plain file reading: the freedesktop spec says where the
//! entries live and what a usable one looks like, and nothing else needs to
//! be consulted. The list is built once at startup and rebuilt whenever one
//! of those directories changes (see `watch`), so opening the launcher never
//! waits on the disk and never shows yesterday's apps.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub comment: String,
    pub icon: String,
    /// The command line, with the spec's field codes already removed.
    pub exec: String,
    pub terminal: bool,
    /// Lowercased name, comment and keywords, for matching without
    /// re-lowercasing on every keystroke.
    #[serde(skip)]
    pub haystack: String,
    /// The name alone, lowercased. Ranking scores it separately and far more
    /// heavily than the rest, and lowercasing it per app per keystroke was
    /// the single largest allocation in the search path.
    #[serde(skip)]
    pub name_lower: String,
    #[serde(skip)]
    pub keywords: String,
}

/// `$XDG_DATA_DIRS` plus the user's own, in the order the spec resolves them:
/// the first entry with a given id wins, so a user override shadows a system
/// one of the same name.
pub fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());

    let mut dirs = vec![data_home.join("applications")];
    dirs.extend(data_dirs.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join("applications")));
    dirs
}

/// Strips the spec's field codes: `%f`, `%U` and friends are placeholders for
/// files the launcher is not passing, and `%%` is a literal percent.
fn clean_exec(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            // Every other code expands to nothing here.
            Some(_) => {}
            None => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The `[Desktop Entry]` group of one file. Other groups are actions, which
/// the launcher does not offer.
fn parse_entry(path: &Path) -> Option<App> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut fields: HashMap<&str, &str> = HashMap::new();
    let mut in_entry = false;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            // Localised keys (Name[de]) are ignored; the launcher shows the
            // unlocalised name, as the shell's own list does.
            let key = key.trim();
            if !key.contains('[') {
                fields.insert(key, value.trim());
            }
        }
    }

    if fields.get("Type") != Some(&"Application") {
        return None;
    }
    if matches!(fields.get("NoDisplay"), Some(&"true")) || matches!(fields.get("Hidden"), Some(&"true")) {
        return None;
    }
    let name = fields.get("Name")?.to_string();
    let exec = clean_exec(fields.get("Exec")?);
    if exec.is_empty() {
        return None;
    }

    let comment = fields.get("Comment").or(fields.get("GenericName")).unwrap_or(&"").to_string();
    let keywords = fields.get("Keywords").unwrap_or(&"").to_lowercase();
    let id = path.file_stem()?.to_string_lossy().into_owned();

    let name_lower = name.to_lowercase();

    Some(App {
        haystack: format!("{} {} {}", name_lower, comment.to_lowercase(), keywords),
        name_lower,
        keywords,
        id,
        name,
        comment,
        icon: fields.get("Icon").unwrap_or(&"").to_string(),
        exec,
        terminal: matches!(fields.get("Terminal"), Some(&"true")),
    })
}

pub fn load() -> Vec<App> {
    let mut seen: HashMap<String, App> = HashMap::new();
    for dir in application_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            if let Some(app) = parse_entry(&path) {
                // First directory wins: the user's own entries shadow the
                // system's.
                seen.entry(app.id.clone()).or_insert(app);
            }
        }
    }
    let mut apps: Vec<App> = seen.into_values().collect();
    apps.sort_by(|a, b| a.name_lower.cmp(&b.name_lower));
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_codes_are_stripped_but_literal_percents_survive() {
        assert_eq!(clean_exec("firefox %u"), "firefox");
        assert_eq!(clean_exec("gimp %U --no-splash"), "gimp --no-splash");
        assert_eq!(clean_exec("foo %%bar"), "foo %bar");
        assert_eq!(clean_exec("code --unity-launch %F"), "code --unity-launch");
    }

    #[test]
    fn this_machine_has_applications() {
        let apps = load();
        assert!(apps.len() > 10, "found only {} desktop entries", apps.len());
        assert!(apps.iter().all(|a| !a.exec.is_empty()));
        assert!(apps.iter().all(|a| !a.name.is_empty()));
    }
}

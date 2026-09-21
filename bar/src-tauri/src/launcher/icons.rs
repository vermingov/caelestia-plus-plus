//! Turning an icon name from a desktop entry into a file the webview can load.
//!
//! The icon theme spec describes a full inheritance-and-size-matching walk.
//! This does the pragmatic version every launcher does: look through the
//! configured theme, then hicolor, then the flat legacy directories, at the
//! sizes that actually exist, and cache what it finds — an icon name resolves
//! once per session, not once per keystroke.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Big enough that the webview never upscales a blurry one, small enough that
/// it is not decoding a 512px PNG for a 22px slot.
const PREFERRED_SIZES: [&str; 8] = ["64x64", "48x48", "96x96", "128x128", "32x32", "256x256", "scalable", "symbolic"];

pub struct Icons {
    roots: Vec<PathBuf>,
    themes: Vec<String>,
    cache: Mutex<HashMap<String, Option<String>>>,
}

fn icon_roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());

    let mut roots = vec![home.join(".icons"), data_home.join("icons")];
    roots.extend(data_dirs.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join("icons")));
    roots
}

/// The GTK icon theme, which is what the rest of the desktop is using. Read
/// from dconf so it follows the scheme the shell sets.
fn configured_theme() -> Option<String> {
    let out = std::process::Command::new("dconf")
        .args(["read", "/org/gnome/desktop/interface/icon-theme"])
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&out.stdout);
    let value = value.trim().trim_matches('\'');
    (!value.is_empty()).then(|| value.to_string())
}

impl Icons {
    pub fn new() -> Icons {
        let mut themes = Vec::new();
        if let Some(theme) = configured_theme() {
            // A Papirus-Dark install keeps most of its icons in the base
            // Papirus directory, so the un-suffixed name has to be searched
            // too or half the list comes back empty.
            if let Some(base) = theme.rsplit_once('-').map(|(base, _)| base.to_string()) {
                themes.push(theme.clone());
                themes.push(base);
            } else {
                themes.push(theme);
            }
        }
        themes.push("hicolor".to_string());
        Icons { roots: icon_roots(), themes, cache: Mutex::new(HashMap::new()) }
    }

    /// Takes over what another resolver has already found, when it was
    /// looking in the same places for the same themes.
    ///
    /// Finding an icon is a walk of the theme's directories, and the answers
    /// outlive the launcher they were found for: it is rebuilt every time it
    /// closes, and without this the next one to open found all of them again,
    /// one keystroke at a time, on the thread that draws.
    #[cfg_attr(feature = "tauri-ui", allow(dead_code))]
    pub fn inherit(&self, other: &Icons) {
        if self.roots != other.roots || self.themes != other.themes {
            return;
        }
        if let (Ok(mut mine), Ok(theirs)) = (self.cache.lock(), other.cache.lock()) {
            mine.extend(theirs.iter().map(|(name, found)| (name.clone(), found.clone())));
        }
    }

    /// Drops everything resolved so far. Called when the desktop entries
    /// change: an install adds icon files too, and a name that resolved to
    /// nothing a moment ago is exactly the name that now has a file.
    pub fn forget(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    pub fn resolve(&self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        // An absolute path in the entry is already the answer.
        if name.starts_with('/') {
            return real_file(Path::new(name));
        }
        if let Some(hit) = self.cache.lock().ok()?.get(name) {
            return hit.clone();
        }
        let found = self.look_up(name);
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(name.to_string(), found.clone());
        }
        found
    }

    fn look_up(&self, name: &str) -> Option<String> {
        for root in &self.roots {
            for theme in &self.themes {
                let theme_dir = root.join(theme);
                if !theme_dir.is_dir() {
                    continue;
                }
                // Themes disagree about whether size or category comes first
                // (48x48/apps versus apps/48), so both layouts are tried.
                for size in PREFERRED_SIZES {
                    for dir in [theme_dir.join(size).join("apps"), theme_dir.join("apps").join(size)] {
                        if let Some(file) = first_match(&dir, name) {
                            return Some(file);
                        }
                    }
                }
                if let Some(file) = deep_search(&theme_dir, name) {
                    return Some(file);
                }
            }
            if let Some(file) = first_match(root, name) {
                return Some(file);
            }
        }
        first_match(Path::new("/usr/share/pixmaps"), name)
    }
}

fn first_match(dir: &Path, name: &str) -> Option<String> {
    ["png", "svg", "xpm"].iter().find_map(|extension| real_file(&dir.join(format!("{name}.{extension}"))))
}

/// The file itself rather than a link to it, which is what the webview has
/// to be given.
///
/// Icon themes alias one icon to another with relative symlinks —
/// `org.xfce.thunar.svg -> system-file-manager.svg` — and Flatpak exports
/// every icon as one. Tauri's asset scope reads such a link's target and
/// tests it as written, against the bar's working directory, where it never
/// exists; the bare name then matches no allowed pattern and the request is
/// refused without a word. Every aliased icon drew as a broken image: Steam
/// from Flatpak, and any app a theme files under another's name.
fn real_file(path: &Path) -> Option<String> {
    let real = std::fs::canonicalize(path).ok()?;
    real.is_file().then(|| real.to_string_lossy().into_owned())
}

/// Last resort for themes that file icons somewhere unusual: two levels of
/// directory under the theme, which covers every layout in the wild without
/// walking a whole theme tree.
fn deep_search(theme_dir: &Path, name: &str) -> Option<String> {
    let entries = std::fs::read_dir(theme_dir).ok()?;
    for entry in entries.flatten() {
        let first = entry.path();
        if !first.is_dir() {
            continue;
        }
        if let Some(file) = first_match(&first, name) {
            return Some(file);
        }
        let Ok(inner) = std::fs::read_dir(&first) else { continue };
        for entry in inner.flatten() {
            let second = entry.path();
            if second.is_dir() {
                if let Some(file) = first_match(&second, name) {
                    return Some(file);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_icons_for_the_apps_on_this_machine() {
        let icons = Icons::new();
        let apps = crate::launcher::apps::load();
        let named: Vec<_> = apps.iter().filter(|a| !a.icon.is_empty()).collect();
        assert!(!named.is_empty(), "no app declares an icon");

        let found = named.iter().filter(|a| icons.resolve(&a.icon).is_some()).count();
        // Some entries name icons their theme genuinely does not ship; most
        // should resolve or the lookup is wrong.
        assert!(
            found * 2 > named.len(),
            "only {found} of {} icons resolved",
            named.len()
        );
    }

    #[test]
    fn an_absolute_path_is_taken_as_given() {
        let icons = Icons::new();
        assert_eq!(icons.resolve("/definitely/not/here.png"), None);
        assert_eq!(icons.resolve(""), None);
    }
}

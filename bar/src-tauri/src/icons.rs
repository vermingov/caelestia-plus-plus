//! Turning an icon name into a file the webview can load.
//!
//! Shared with the launcher, which has the same problem from the other end: it
//! resolves desktop entries, the bar resolves whatever a tray item calls its
//! icon.
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

    pub fn resolve(&self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        // An absolute path in the entry is already the answer.
        if name.starts_with('/') {
            return Path::new(name).is_file().then(|| name.to_string());
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
    for extension in ["png", "svg", "xpm"] {
        let candidate = dir.join(format!("{name}.{extension}"));
        if candidate.is_file() {
            // Resolved, not as written: icon themes are built out of symlinks
            // — Papirus files one icon and points a dozen names at it — and
            // the webview's asset protocol will not follow one.
            return Some(
                candidate
                    .canonicalize()
                    .unwrap_or(candidate)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    None
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

    /// The bar has no app list to test against — that is the launcher's half
    /// of this file — so it tests the lookup against icons every desktop has.
    #[test]
    fn resolves_icons_the_desktop_is_certain_to_have() {
        let icons = Icons::new();
        let found = ["folder", "text-x-generic", "audio-volume-high"]
            .iter()
            .filter(|name| icons.resolve(name).is_some())
            .count();
        assert!(found > 0, "no stock icon resolved; the theme walk is wrong");
    }

    #[test]
    fn an_absolute_path_is_taken_as_given() {
        let icons = Icons::new();
        assert_eq!(icons.resolve("/definitely/not/here.png"), None);
        assert_eq!(icons.resolve(""), None);
    }
}


/// The one lookup the tray uses, built on first use.
///
/// A tray icon is resolved from a background thread that has nothing to hang
/// an `Icons` on, and the set of themes cannot change without the session
/// restarting anyway.
pub fn lookup(name: &str) -> Option<String> {
    use std::sync::OnceLock;
    static ICONS: OnceLock<Icons> = OnceLock::new();
    ICONS.get_or_init(Icons::new).resolve(name)
}

/// An icon from a directory the item named itself, which takes precedence
/// over the theme: an application that ships its own icons and says where
/// they are has told us exactly what it wants drawn.
pub fn in_directory(directory: &str, name: &str) -> Option<String> {
    let root = Path::new(directory);
    first_match(root, name).or_else(|| deep_search(root, name))
}

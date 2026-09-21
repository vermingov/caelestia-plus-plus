//! The colour scheme that is set, and setting parts of it.
//!
//! The CLI owns schemes: it writes what is set to a state file and rewrites
//! every themed config when that changes. Reading the file says what is set
//! without starting anything; changing it is asked of the CLI, detached,
//! because regenerating a palette outlasts any window that asked for it.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Current {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub flavour: String,
    /// `dark` or `light`.
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub variant: String,
}

impl Current {
    pub fn is_dark(&self) -> bool {
        self.mode != "light"
    }
}

fn state_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let state = std::env::var_os("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from);
    Some(state.join("caelestia/scheme.json"))
}

pub fn current() -> Current {
    let text = state_file().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

/// The scheme's primary colour, as the six digits the file keeps it in.
pub fn primary() -> Option<String> {
    let text = state_file().and_then(|path| std::fs::read_to_string(path).ok())?;
    let set: serde_json::Value = serde_json::from_str(&text).ok()?;
    set.get("colours")?.get("primary")?.as_str().map(str::to_string)
}

pub fn set_dark(dark: bool) {
    let mode = if dark { "dark" } else { "light" };
    let _ = std::process::Command::new("setsid").args(["-f", "caelestia", "scheme", "set", "-m", mode]).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_file_is_read_for_what_is_set_and_not_for_its_colours() {
        let set: Current =
            serde_json::from_str(r#"{"name": "red", "flavour": "default", "mode": "light", "variant": "vibrant", "colours": {"primary": "ff5449"}}"#)
                .unwrap();
        assert_eq!((set.name.as_str(), set.variant.as_str()), ("red", "vibrant"));
        assert!(!set.is_dark());
        assert!(Current::default().is_dark(), "a desktop that says nothing is a dark one");
    }
}

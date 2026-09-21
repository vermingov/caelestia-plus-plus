//! The compositor's settings, through the helper that owns them.
//!
//! `caelestia-tools hyprmod` edits the Hyprland config where it lies: the
//! knobs in `variables.lua` in place, so the file keeps its comments, and
//! everything else in a state file it turns into Lua. It also applies what it
//! can without a reload. All of that stays its job. This only asks it, so
//! that the QML settings and these can never disagree about what a knob is.

use serde::Deserialize;
use serde_json::{Map, Value};

const HELPER: &str = "caelestia-tools";

fn ask(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(HELPER).arg("hyprmod").args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn tell(args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new(HELPER)
        .arg("hyprmod")
        .args(args)
        .output()
        .map_err(|error| format!("cannot run {HELPER}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

/// The curated knobs by name, as an object. Nothing at all when the helper is
/// not installed or the Hyprland config is not one it knows its way around.
pub fn knobs() -> Value {
    ask(&["dump"]).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

pub fn set(key: &str, value: &str) -> Result<(), String> {
    tell(&["set", key, value])
}

/// One of Hyprland's own options, as `hyprctl descriptions` lists it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct HyprOption {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub current: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Every option the running compositor has: a few hundred of them.
pub fn schema() -> Vec<HyprOption> {
    ask(&["schema"]).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Bind {
    pub combo: String,
    pub kind: String,
    pub value: String,
}

/// What has been layered over the base config: option overrides, custom
/// keybinds, and where each monitor was put.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Overrides {
    #[serde(default)]
    pub options: Map<String, Value>,
    #[serde(default)]
    pub binds: Vec<Bind>,
    #[serde(default)]
    pub monitors: Map<String, Value>,
    #[serde(default)]
    pub primary: String,
}

pub fn overrides() -> Overrides {
    ask(&["overrides"]).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn set_option(name: &str, value: &str) -> Result<(), String> {
    tell(&["set-option", name, value])
}

pub fn unset_option(name: &str) -> Result<(), String> {
    tell(&["unset-option", name])
}

pub fn add_bind(combo: &str, kind: &str, value: &str) -> Result<(), String> {
    tell(&["add-bind", combo, kind, value])
}

pub fn remove_bind(index: usize) -> Result<(), String> {
    tell(&["del-bind", &index.to_string()])
}

/// `spec` is the monitor's mode, position and scale, as a JSON object.
pub fn set_monitor(name: &str, spec: &Value) -> Result<(), String> {
    tell(&["set-monitor", name, &spec.to_string()])
}

pub fn forget_monitor(name: &str) -> Result<(), String> {
    tell(&["del-monitor", name])
}

/// The monitor workspace 1 lives on after a restart. An empty name is none.
pub fn set_primary(name: &str) -> Result<(), String> {
    tell(&["set-primary", name])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helpers_listings_are_read_as_they_are_written() {
        let option: HyprOption = serde_json::from_str(
            r#"{"name": "general:border_size", "description": "size of the border", "default": 1, "current": 1, "min": 0, "max": 20, "map": null}"#,
        )
        .unwrap();
        assert_eq!((option.min, option.max), (Some(0.), Some(20.)));

        // A string option has no range, and says so with nulls.
        let option: HyprOption =
            serde_json::from_str(r#"{"name": "general:gaps_in", "default": "5 5 5 5", "current": "5 5 5 5", "min": null, "max": null}"#)
                .unwrap();
        assert_eq!(option.min, None);
        assert_eq!(option.description, "");
    }

    #[test]
    fn overrides_with_nothing_in_them_are_nothing() {
        let state: Overrides = serde_json::from_str(r#"{"binds": [], "monitors": {}, "options": {}, "primary": ""}"#).unwrap();
        assert_eq!(state, Overrides::default());

        let state: Overrides = serde_json::from_str(
            r#"{"binds": [{"combo": "SUPER + B", "kind": "exec", "value": "firefox", "flags": []}], "monitors": {"eDP-1": {"scale": 1}}}"#,
        )
        .unwrap();
        assert_eq!(state.binds[0].combo, "SUPER + B");
        assert!(state.monitors.contains_key("eDP-1"));
    }
}

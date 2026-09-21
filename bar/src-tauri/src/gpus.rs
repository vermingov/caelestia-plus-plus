//! Which graphics card an application is started on.
//!
//! A machine with two cards runs everything on the frugal one unless told
//! otherwise, and what tells it is a few environment variables set for the
//! one program. Which program gets which card is kept by desktop entry id and
//! by the card's PCI slot, which stays put across restarts where the number
//! on its render node does not. The file is the one the QML shell kept, so
//! what was assigned there still is.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub driver: String,
    /// As `lspci` writes it: `0000:74:00.0`.
    pub slot: String,
}

/// The cards with a render node, from the shell's helper.
pub fn list() -> Vec<Gpu> {
    let listed = std::process::Command::new("caelestia-tools").arg("gpus").output();
    listed.ok().and_then(|output| serde_json::from_slice(&output.stdout).ok()).unwrap_or_default()
}

fn file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let state = std::env::var_os("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from);
    Some(state.join("caelestia/app-gpus.json"))
}

/// Desktop entry id to PCI slot, for every application that has been given
/// a card of its own.
pub fn assignments() -> BTreeMap<String, String> {
    let text = file().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

/// Gives `app` the card in `slot`, or with no slot, takes its card away and
/// leaves it to the machine's default.
pub fn assign(app: &str, slot: Option<&str>) -> Result<(), String> {
    let path = file().ok_or("no home directory to keep the assignment in")?;
    let mut assigned = assignments();
    match slot {
        Some(slot) => assigned.insert(app.to_string(), slot.to_string()),
        None => assigned.remove(app),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&assigned).map_err(|error| error.to_string())?;
    let beside = path.with_extension("json.new");
    std::fs::write(&beside, text + "\n").map_err(|error| error.to_string())?;
    std::fs::rename(&beside, &path).map_err(|error| error.to_string())
}

/// What to set for a program to run on `gpu`. Mesa's drivers are told which
/// card by its slot. NVIDIA's own driver is not told which, only that it is
/// wanted, in the three places it looks.
pub fn environment(gpu: &Gpu) -> Vec<(&'static str, String)> {
    if gpu.driver == "nvidia" {
        return vec![
            ("__NV_PRIME_RENDER_OFFLOAD", "1".to_string()),
            ("__GLX_VENDOR_LIBRARY_NAME", "nvidia".to_string()),
            ("__VK_LAYER_NV_optimus", "NVIDIA_only".to_string()),
        ];
    }
    vec![("DRI_PRIME", format!("pci-{}", gpu.slot.replace([':', '.'], "_")))]
}

/// `env A=1 B=2 ` to go in front of `app`'s command line, or nothing when it
/// has no card of its own. Read when a program is started rather than kept:
/// it is two small files, and starting a program is already a moment's work.
pub fn launch_prefix(app: &str) -> String {
    let Some(slot) = assignments().remove(app) else { return String::new() };
    let Some(gpu) = list().into_iter().find(|gpu| gpu.slot == slot) else { return String::new() };
    let set: Vec<String> = environment(&gpu).into_iter().map(|(name, value)| format!("{name}={value}")).collect();
    format!("env {} ", set.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_driver_is_told_in_its_own_way() {
        let radeon = Gpu { name: "Radeon 680M".into(), driver: "amdgpu".into(), slot: "0000:74:00.0".into() };
        assert_eq!(environment(&radeon), [("DRI_PRIME", "pci-0000_74_00_0".to_string())]);

        let nvidia = Gpu { name: "RTX 4070".into(), driver: "nvidia".into(), slot: "0000:01:00.0".into() };
        let set = environment(&nvidia);
        assert!(set.contains(&("__NV_PRIME_RENDER_OFFLOAD", "1".to_string())));
        assert!(!set.iter().any(|(name, _)| *name == "DRI_PRIME"), "NVIDIA's driver does not read Mesa's variable");
    }

    #[test]
    fn the_helpers_listing_is_read() {
        let listed: Vec<Gpu> =
            serde_json::from_str(r#"[{"driver":"amdgpu","name":"Radeon 680M","node":"/dev/dri/renderD128","slot":"0000:74:00.0"}]"#).unwrap();
        assert_eq!(listed[0].slot, "0000:74:00.0");
    }
}

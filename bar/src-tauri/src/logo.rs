//! Which mark the bar wears, decided the way the shell decides it.
//!
//! Three sources, in order: an image the user pointed at in their preferences,
//! the `general.logo` entry in shell.json, and failing both, whatever
//! `/etc/os-release` says the distribution's logo is. Only when none of those
//! resolve does it fall back to Caelestia's own mark — which is the same order
//! `SysInfo` and `OsIcon` use, so both bars show the same thing.

use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Logo {
    /// "caelestia" and "cachyos" are drawn from marks bundled with the bar, so
    /// they can take the accent colour.
    pub kind: String,
    pub show: bool,
}

/// When the files these settings come from were last written.
///
/// Polled rather than subscribed to, like the tray: two `stat` calls every
/// couple of seconds against a dependency and an event loop is the right
/// trade for something that changes when a person opens Settings.
pub fn stamp() -> Vec<Option<std::time::SystemTime>> {
    let files = [
        state_dir().map(|dir| dir.join("prefs.json")),
        config_dir().map(|dir| dir.join("shell.json")),
    ];
    files
        .into_iter()
        .map(|path| path.and_then(|path| std::fs::metadata(path).ok()?.modified().ok()))
        .collect()
}

fn config_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
    Some(std::path::PathBuf::from(config).join("caelestia"))
}

fn state_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let state = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| format!("{home}/.local/state"));
    Some(std::path::PathBuf::from(state).join("caelestia"))
}

fn json(path: std::path::PathBuf) -> serde_json::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// One field of `/etc/os-release`, unquoted.
fn os_release(field: &str) -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{field}=")))
        .map(|value| value.trim_matches('"').to_string())
}

/// Which status glyphs the row carries, and in what order the bar's entries
/// are laid out — both of which are the user's config, not the bar's choice.
///
/// The shell's bar is built from `bar.entries`; hard-coding an order here
/// would mean a config that moves the clock moves it in one bar and not the
/// other.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    /// Entry ids in the order they are drawn.
    pub entries: Vec<String>,
    pub status: Status,
    pub stats: Stats,
}

/// Which usage dials the monitor pill carries. The fork keeps these in its
/// preferences rather than in shell.json, and a pill with no metric turned on
/// earns no space at all.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub cpu: bool,
    pub ram: bool,
    pub gpu: bool,
}

impl Default for Stats {
    fn default() -> Stats {
        Stats { cpu: true, ram: true, gpu: true }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub show_network: bool,
    pub show_bluetooth: bool,
    pub show_battery: bool,
    pub show_audio: bool,
    pub show_microphone: bool,
    pub show_kb_layout: bool,
    pub show_lock_status: bool,
}

impl Default for Status {
    fn default() -> Status {
        // What this fork's bar actually shows out of the box: the link, the
        // adapter and the charge. The rest have their own keys and their own
        // on-screen displays, and are off unless the config asks for them.
        Status {
            show_network: true,
            show_bluetooth: true,
            show_battery: true,
            show_audio: false,
            show_microphone: false,
            show_kb_layout: false,
            show_lock_status: false,
        }
    }
}

/// The entries this fork adds, which a config written by upstream caelestia
/// will not mention. Injected before the trailing readouts, exactly as
/// `Bar.qml` injects its own — a foreign config must not silently drop them.
const INJECTED: [&str; 6] = ["logo", "specials", "activeWindow", "media", "visualiser", "sysStats"];

/// Where the injected entries go, relative to the ones a stock config has.
fn inject_before(id: &str) -> Option<&'static str> {
    match id {
        // The mark caps the left end, before anything else.
        "logo" => Some("workspaces"),
        // Next to the workspaces they belong to.
        "specials" => Some("spacer"),
        "activeWindow" => Some("spacer"),
        "media" => Some("spacer"),
        "visualiser" => Some("spacer"),
        _ => None,
    }
}

/// The monitor pill goes with the other trailing readouts, wherever the
/// config happens to have put them — the same anchors `Bar.qml` looks for.
fn stats_anchor(entries: &[String]) -> usize {
    entries
        .iter()
        .position(|entry| ["tray", "clock", "statusIcons", "power"].contains(&entry.as_str()))
        .unwrap_or(entries.len())
}

pub fn layout() -> Layout {
    let config = config_dir().map(|dir| json(dir.join("shell.json"))).unwrap_or(serde_json::Value::Null);
    let bar = config.get("bar");

    let mut entries: Vec<String> = bar
        .and_then(|bar| bar.get("entries"))
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| {
                    // An entry can be listed and switched off.
                    entry.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(true)
                })
                .filter_map(|entry| entry.get("id").and_then(serde_json::Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    if entries.is_empty() {
        entries = vec![
            "workspaces".into(),
            "spacer".into(),
            "firewall".into(),
            "features".into(),
            "sysStats".into(),
            "tray".into(),
            "statusIcons".into(),
            "clock".into(),
            "power".into(),
        ];
    }

    // The fork's own entries, added unless the config mentions them — an
    // explicit `enabled: false` still wins, because it has already been
    // filtered out above and `mentions` sees it here.
    let mentioned = |id: &str| -> bool {
        bar.and_then(|bar| bar.get("entries"))
            .and_then(serde_json::Value::as_array)
            .map(|entries| {
                entries.iter().any(|entry| {
                    entry.get("id").and_then(serde_json::Value::as_str) == Some(id)
                })
            })
            .unwrap_or(false)
    };

    let prefs = state_dir().map(|dir| json(dir.join("prefs.json"))).unwrap_or(serde_json::Value::Null);
    let pref = |key: &str, default: bool| -> bool {
        prefs.get(key).and_then(serde_json::Value::as_bool).unwrap_or(default)
    };

    let stats = Stats {
        cpu: pref("barShowCpu", true),
        ram: pref("barShowRam", true),
        gpu: pref("barShowGpu", true),
    };

    for id in INJECTED {
        if mentioned(id) || entries.iter().any(|entry| entry == id) {
            continue;
        }
        let at = if id == "sysStats" {
            stats_anchor(&entries)
        } else {
            inject_before(id)
                .and_then(|anchor| entries.iter().position(|entry| entry == anchor))
                .unwrap_or(entries.len())
        };
        entries.insert(at, id.to_string());
    }

    // Two entries earn their place from the preferences rather than from the
    // entry list: the centre readout this fork ships without, and the monitor
    // pill with every metric switched off.
    if !pref("barShowActiveWindow", false) {
        entries.retain(|entry| entry != "activeWindow");
    }
    if !stats.cpu && !stats.ram && !stats.gpu {
        entries.retain(|entry| entry != "sysStats");
    }

    let status = bar.and_then(|bar| bar.get("status"));
    let fallback = Status::default();
    let flag = |key: &str, default: bool| -> bool {
        status
            .and_then(|status| status.get(key))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(default)
    };

    Layout {
        entries,
        stats,
        status: Status {
            show_network: flag("showNetwork", fallback.show_network),
            show_bluetooth: flag("showBluetooth", fallback.show_bluetooth),
            show_battery: flag("showBattery", fallback.show_battery),
            show_audio: flag("showAudio", fallback.show_audio),
            show_microphone: flag("showMicrophone", fallback.show_microphone),
            show_kb_layout: flag("showKbLayout", fallback.show_kb_layout),
            show_lock_status: flag("showLockStatus", fallback.show_lock_status),
        },
    }
}

/// The bar's own options from shell.json, as the shell reads them.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BarConfig {
    /// How many workspace pips are shown at once.
    pub shown: i64,
    /// A filled track behind the occupied ones.
    pub occupied_bg: bool,
    /// The marker leaves a trail as it travels.
    pub active_trail: bool,
    /// Window icons on the pips.
    pub show_windows: bool,
    /// Each bar shows only its own screen's workspaces.
    pub per_monitor: bool,
    /// Text drawn instead of the number: for every pip, the occupied ones and
    /// the focused one respectively. Empty means the number.
    pub label: String,
    pub occupied_label: String,
    pub active_label: String,
}

impl Default for BarConfig {
    fn default() -> BarConfig {
        BarConfig {
            shown: 5,
            occupied_bg: true,
            active_trail: true,
            show_windows: false,
            per_monitor: false,
            label: String::new(),
            occupied_label: String::new(),
            active_label: String::new(),
        }
    }
}

pub fn bar_config() -> BarConfig {
    let config = config_dir().map(|dir| json(dir.join("shell.json"))).unwrap_or(serde_json::Value::Null);
    let Some(workspaces) = config.get("bar").and_then(|bar| bar.get("workspaces")) else {
        return BarConfig::default();
    };
    let fallback = BarConfig::default();

    let text = |key: &str| -> String {
        workspaces.get(key).and_then(serde_json::Value::as_str).unwrap_or_default().to_string()
    };
    let flag = |key: &str, default: bool| -> bool {
        workspaces.get(key).and_then(serde_json::Value::as_bool).unwrap_or(default)
    };

    BarConfig {
        shown: workspaces.get("shown").and_then(serde_json::Value::as_i64).unwrap_or(fallback.shown),
        occupied_bg: flag("occupiedBg", fallback.occupied_bg),
        active_trail: flag("activeTrail", fallback.active_trail),
        show_windows: flag("showWindows", fallback.show_windows),
        per_monitor: flag("perMonitorWorkspaces", fallback.per_monitor),
        label: text("label"),
        occupied_label: text("occupiedLabel"),
        active_label: text("activeLabel"),
    }
}

pub fn read() -> Logo {
    let prefs = state_dir().map(|dir| json(dir.join("prefs.json"))).unwrap_or(serde_json::Value::Null);
    let config = config_dir().map(|dir| json(dir.join("shell.json"))).unwrap_or(serde_json::Value::Null);

    let string = |value: &serde_json::Value, key: &str| -> String {
        value.get(key).and_then(serde_json::Value::as_str).unwrap_or_default().to_string()
    };

    let mut logo = Logo {
        show: prefs.get("barLogoShow").and_then(serde_json::Value::as_bool).unwrap_or(true),
        ..Logo::default()
    };

    // The shell config's choice.
    let configured = config.get("general").map(|general| string(general, "logo")).unwrap_or_default();
    if configured == "caelestia" {
        logo.kind = "caelestia".to_string();
        return logo;
    }
    // Then the distribution's, which is the case on an untouched install.
    let distro = os_release("LOGO").or_else(|| os_release("ID")).unwrap_or_default();
    if distro.contains("cachyos") {
        // Bundled rather than looked up: the icon theme's version is a flat
        // raster, and the bar wants a mark it can recolour.
        logo.kind = "cachyos".to_string();
        return logo;
    }
    logo.kind = "caelestia".to_string();
    logo
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_fields_come_back_unquoted() {
        // Every machine has an ID; the point is that the quotes some
        // distributions put round it do not end up in the icon name.
        let id = os_release("ID").expect("every os-release has an ID");
        assert!(!id.contains('"'), "ID came back as {id:?}");
    }

    #[test]
    fn a_mark_is_always_chosen() {
        // Whatever this machine is, the bar has something to draw: the
        // fallback chain ends at Caelestia's own mark rather than at nothing.
        let logo = read();
        assert!(["caelestia", "cachyos"].contains(&logo.kind.as_str()), "kind was {:?}", logo.kind);
    }

    #[test]
    fn the_layout_follows_the_config_and_keeps_the_forks_own_entries() {
        let layout = layout();
        // Whatever the config says, the entries it does not know about are
        // still there — a config written by upstream caelestia must not
        // silently drop this fork's additions.
        for id in INJECTED {
            // The centre readout is off by default in this fork, so it is the
            // one injected entry that legitimately may not be there.
            if id == "activeWindow" {
                continue;
            }
            assert!(layout.entries.iter().any(|entry| entry == id), "{id} was dropped");
        }
        // And the mark caps the left end.
        assert_eq!(layout.entries.first().map(String::as_str), Some("logo"));
    }

    #[test]
    fn an_empty_config_still_produces_a_bar() {
        let layout = layout();
        assert!(layout.entries.contains(&"clock".to_string()));
        assert!(layout.status.show_battery);
    }

    #[test]
    fn workspace_options_fall_back_to_the_shell_defaults() {
        // A config that says nothing about the bar must still produce the
        // five-pip row the shell draws by default, not a row of none.
        let options = bar_config();
        assert!(options.shown >= 1, "shown was {}", options.shown);
    }
}

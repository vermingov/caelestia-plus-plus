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

/// When the files these settings come from were last written, and the two
/// beside them that say what the desktop looks like: which wallpaper, and
/// which scheme.
///
/// Polled rather than subscribed to, like the tray: a few `stat` calls every
/// couple of seconds against a dependency and an event loop is the right
/// trade for something that changes when a person opens Settings.
pub fn stamp() -> Vec<Option<std::time::SystemTime>> {
    let files = [
        state_dir().map(|dir| dir.join("prefs.json")),
        config_dir().map(|dir| dir.join("shell.json")),
        state_dir().map(|dir| dir.join("wallpaper/path.txt")),
        state_dir().map(|dir| dir.join("scheme.json")),
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

/// What a bar with nothing said about it carries.
const STOCK: [&str; 9] = ["workspaces", "spacer", "firewall", "features", "sysStats", "tray", "statusIcons", "clock", "power"];

/// Every entry the bar could draw, in the order it would draw them, and
/// whether each is switched on.
///
/// What the config lists, as it lists it, with this fork's own entries put
/// in where they belong unless the config mentions them: an entry can be
/// listed and switched off, and that is a mention. This is the list a
/// settings page shows, so that switching one entry off and on again leaves
/// it where it was.
pub fn entries_of(bar_entries: Option<&serde_json::Value>) -> Vec<(String, bool)> {
    let mut entries: Vec<(String, bool)> = bar_entries
        .and_then(serde_json::Value::as_array)
        .map(|listed| {
            listed
                .iter()
                .filter_map(|entry| {
                    let id = entry.get("id").and_then(serde_json::Value::as_str)?;
                    let enabled = entry.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(true);
                    Some((id.to_string(), enabled))
                })
                .collect()
        })
        .unwrap_or_default();

    if entries.is_empty() {
        entries = STOCK.iter().map(|id| (id.to_string(), true)).collect();
    }

    for id in INJECTED {
        if entries.iter().any(|(entry, _)| entry == id) {
            continue;
        }
        let ids: Vec<String> = entries.iter().map(|(entry, _)| entry.clone()).collect();
        let at = if id == "sysStats" {
            stats_anchor(&ids)
        } else {
            inject_before(id).and_then(|anchor| ids.iter().position(|entry| entry == anchor)).unwrap_or(ids.len())
        };
        entries.insert(at, (id.to_string(), true));
    }
    entries
}

/// `bar.entries` with one entry switched on or off, as it should be written
/// back. The whole resolved list is written, this fork's entries included:
/// an entry that was only ever implied has no `enabled` to set.
pub fn entries_with(bar_entries: Option<&serde_json::Value>, id: &str, enabled: bool) -> serde_json::Value {
    let listed = bar_entries.and_then(serde_json::Value::as_array);
    let mut spent = vec![false; listed.map_or(0, Vec::len)];

    let written = entries_of(bar_entries).into_iter().map(|(entry, was_enabled)| {
        // The object the config already had for it, which may say more than
        // this shell knows to ask about. There can be several spacers, so
        // each object answers for one entry only.
        let original = listed.and_then(|listed| {
            let at = listed.iter().enumerate().position(|(at, object)| {
                !spent[at] && object.get("id").and_then(serde_json::Value::as_str) == Some(entry.as_str())
            })?;
            spent[at] = true;
            listed[at].as_object().cloned()
        });
        let mut object = original.unwrap_or_default();
        object.insert("id".to_string(), entry.clone().into());
        object.insert("enabled".to_string(), (if entry == id { enabled } else { was_enabled }).into());
        serde_json::Value::Object(object)
    });
    written.collect()
}

pub fn layout() -> Layout {
    let config = config_dir().map(|dir| json(dir.join("shell.json"))).unwrap_or(serde_json::Value::Null);
    let bar = config.get("bar");

    let mut entries: Vec<String> = entries_of(bar.and_then(|bar| bar.get("entries")))
        .into_iter()
        .filter_map(|(id, enabled)| enabled.then_some(id))
        .collect();

    let prefs = state_dir().map(|dir| json(dir.join("prefs.json"))).unwrap_or(serde_json::Value::Null);
    let pref = |key: &str, default: bool| -> bool {
        prefs.get(key).and_then(serde_json::Value::as_bool).unwrap_or(default)
    };

    let stats = Stats {
        cpu: pref("barShowCpu", true),
        ram: pref("barShowRam", true),
        gpu: pref("barShowGpu", true),
    };

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

/// The notification options from shell.json, with the shell's own defaults.
///
/// Read per notification rather than cached: it changes when somebody moves a
/// slider in the settings, and a bar that had to restart to notice would be a
/// worse bar than the QML one it replaced.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifsConfig {
    /// Toasts leave the screen when their time is up. Off, they stay until
    /// somebody dismisses them, except over a fullscreen window, where
    /// nothing is allowed to sit for ever. Either way the notification itself
    /// stays in the list: this is about the toast, not the history.
    pub expire: bool,
    /// Whether anything is shown over a fullscreen window at all.
    pub fullscreen: bool,
    /// Milliseconds a toast stays up when the sender does not say.
    pub default_expire_timeout: u64,
    /// The same, over a fullscreen window: shorter, because it is covering
    /// something somebody is watching.
    pub fullscreen_expire_timeout: u64,
    /// How many of a group are shown before it collapses.
    pub group_preview_num: i64,
    /// Notifications open with their body already unfolded.
    pub open_expanded: bool,
    /// A click on a toast with exactly one action presses that action.
    pub action_on_click: bool,
    /// How far across its own width something has to be dragged before
    /// letting go throws it away.
    pub clear_threshold: f64,
    /// How far up or down a drag has to go, in pixels, to fold or unfold.
    pub expand_threshold: f64,
}

impl Default for NotifsConfig {
    fn default() -> NotifsConfig {
        // The plugin's own defaults (notifsconfig.hpp), so that a shell.json
        // that says nothing about notifications means the same thing to this
        // server as it did to the shell's.
        NotifsConfig {
            expire: true,
            fullscreen: true,
            default_expire_timeout: 5000,
            fullscreen_expire_timeout: 2000,
            group_preview_num: 3,
            open_expanded: false,
            action_on_click: false,
            clear_threshold: 0.3,
            expand_threshold: 20.0,
        }
    }
}

pub fn notifs_config() -> NotifsConfig {
    let config = config_dir().map(|dir| json(dir.join("shell.json"))).unwrap_or(serde_json::Value::Null);
    let fallback = NotifsConfig::default();
    let Some(notifs) = config.get("notifs") else { return fallback };

    let flag = |key: &str, default: bool| -> bool {
        notifs.get(key).and_then(serde_json::Value::as_bool).unwrap_or(default)
    };
    let number = |key: &str, default: u64| -> u64 {
        notifs.get(key).and_then(serde_json::Value::as_u64).unwrap_or(default)
    };

    NotifsConfig {
        expire: flag("expire", fallback.expire),
        // Spelled as a word in the config — "off" or "on" — because it was a
        // three-way setting once and the shell still writes it that way.
        fullscreen: notifs
            .get("fullscreen")
            .and_then(serde_json::Value::as_str)
            .map(|mode| mode != "off")
            .unwrap_or(fallback.fullscreen),
        default_expire_timeout: number("defaultExpireTimeout", fallback.default_expire_timeout),
        fullscreen_expire_timeout: number("fullscreenExpireTimeout", fallback.fullscreen_expire_timeout),
        group_preview_num: notifs
            .get("groupPreviewNum")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(fallback.group_preview_num),
        open_expanded: flag("openExpanded", fallback.open_expanded),
        action_on_click: flag("actionOnClick", fallback.action_on_click),
        clear_threshold: notifs
            .get("clearThreshold")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(fallback.clear_threshold),
        expand_threshold: notifs
            .get("expandThreshold")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(fallback.expand_threshold),
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

    /// This machine's own config: a stock list, which says nothing of what
    /// the fork adds.
    fn stock_config() -> serde_json::Value {
        serde_json::json!([
            {"id": "workspaces"}, {"id": "spacer"}, {"id": "spacer"}, {"id": "firewall"},
            {"id": "features"}, {"id": "tray"}, {"id": "clock"}, {"id": "statusIcons"}, {"id": "power"}
        ])
    }

    fn ids(entries: &[(String, bool)]) -> Vec<&str> {
        entries.iter().map(|(id, _)| id.as_str()).collect()
    }

    #[test]
    fn the_forks_entries_are_put_where_they_belong() {
        let entries = entries_of(Some(&stock_config()));
        assert_eq!(
            ids(&entries),
            [
                "logo", "workspaces", "specials", "activeWindow", "media", "visualiser", "spacer", "spacer", "firewall",
                "features", "sysStats", "tray", "clock", "statusIcons", "power"
            ]
        );
        assert!(entries.iter().all(|(_, enabled)| *enabled));
    }

    /// The point of writing the whole list back: an entry that was only
    /// implied has a place once it has been switched off, and switched on
    /// again it is still there rather than at the far end of the bar.
    #[test]
    fn an_entry_switched_off_and_on_again_stays_where_it_was() {
        let before = ids(&entries_of(Some(&stock_config()))).join(" ");

        let off = entries_with(Some(&stock_config()), "media", false);
        let listed = entries_of(Some(&off));
        assert_eq!(listed.iter().find(|(id, _)| id == "media"), Some(&("media".to_string(), false)));
        assert_eq!(ids(&listed).join(" "), before, "switching an entry off moved something");

        let on = entries_with(Some(&off), "media", true);
        let listed = entries_of(Some(&on));
        assert!(listed.iter().all(|(_, enabled)| *enabled));
        assert_eq!(ids(&listed).join(" "), before, "switching it back on moved something");
    }

    #[test]
    fn what_the_config_said_about_an_entry_is_kept_when_another_is_switched() {
        let config = serde_json::json!([{"id": "clock", "somethingElse": 3}, {"id": "spacer"}, {"id": "spacer"}, {"id": "power"}]);
        let written = entries_with(Some(&config), "power", false);

        let clock = written.as_array().unwrap().iter().find(|entry| entry["id"] == "clock").unwrap();
        assert_eq!(clock["somethingElse"], 3, "a key this shell does not know was dropped");
        let spacers = written.as_array().unwrap().iter().filter(|entry| entry["id"] == "spacer").count();
        assert_eq!(spacers, 2, "two spacers are two entries");
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

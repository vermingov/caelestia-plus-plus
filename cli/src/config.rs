//! The toggle configuration: which apps belong to which special workspace.
//!
//! The Python CLI carries these defaults in code and lets `cli.json` override
//! them per app, chaining the two maps so a user who changes one field of one
//! app keeps the rest. This does the same by merging, which is the same thing
//! for a config read once and thrown away.

use redcommon::json::{self, Json};

/// `~/.config/caelestia/cli.json`, or an empty object when there is none.
pub fn user_config() -> Json {
    std::fs::read_to_string(crate::paths::user_config_path())
        .ok()
        .and_then(|text| json::parse(&text))
        .unwrap_or_else(|| json::obj([]))
}

/// The toggle map: defaults with the user's overrides merged over the top.
pub fn toggles() -> Json {
    let defaults = default_toggles();
    match user_config().get("toggles") {
        Some(user) => deep_merge(&defaults, user),
        None => defaults,
    }
}

/// `over` wins, key by key, recursing into objects so a partial override
/// keeps the fields it does not mention.
fn deep_merge(base: &Json, over: &Json) -> Json {
    let (Json::Obj(base_map), Json::Obj(over_map)) = (base, over) else {
        return over.clone();
    };
    let mut merged = base_map.clone();
    for (key, value) in over_map {
        let combined = match merged.get(key) {
            Some(existing) => deep_merge(existing, value),
            None => value.clone(),
        };
        merged.insert(key.clone(), combined);
    }
    Json::Obj(merged)
}

fn app(matches: Json, command: Option<Json>, move_it: bool) -> Json {
    let mut fields = vec![
        ("enable", Json::Bool(true)),
        ("match", matches),
        ("move", Json::Bool(move_it)),
    ];
    if let Some(command) = command {
        fields.push(("command", command));
    }
    Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn class(name: &str) -> Json {
    json::obj([("class", json::s(name))])
}

fn command(parts: &[&str]) -> Json {
    Json::Arr(parts.iter().map(|p| json::s(*p)).collect())
}

fn default_toggles() -> Json {
    json::obj([
        (
            "communication",
            json::obj([
                (
                    "discord",
                    app(Json::Arr(vec![class("discord")]), Some(command(&["discord"])), true),
                ),
                ("whatsapp", app(Json::Arr(vec![class("whatsapp")]), None, true)),
            ]),
        ),
        (
            "music",
            json::obj([
                (
                    "spotify",
                    app(
                        Json::Arr(vec![
                            class("Spotify"),
                            json::obj([("initialTitle", json::s("Spotify"))]),
                            json::obj([("initialTitle", json::s("Spotify Free"))]),
                        ]),
                        Some(command(&["spicetify", "watch", "-s"])),
                        true,
                    ),
                ),
                ("feishin", app(Json::Arr(vec![class("feishin")]), None, true)),
            ]),
        ),
        (
            "sysmon",
            json::obj([(
                "btop",
                app(
                    Json::Arr(vec![json::obj([
                        ("class", json::s("btop")),
                        ("title", json::s("btop")),
                        ("workspace", json::obj([("name", json::s("special:sysmon"))])),
                    ])]),
                    Some(command(&["foot", "-a", "btop", "-T", "btop", "fish", "-C", "exec btop"])),
                    false,
                ),
            )]),
        ),
        (
            "todo",
            json::obj([(
                "todoist",
                app(
                    Json::Arr(vec![class("Todoist")]),
                    Some(command(&["todoist"])),
                    true,
                ),
            )]),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_describe_every_workspace_the_keybinds_toggle() {
        let toggles = default_toggles();
        for workspace in ["communication", "music", "sysmon", "todo"] {
            assert!(toggles.get(workspace).is_some(), "{workspace} missing");
        }
        let btop = toggles.get("sysmon").unwrap().get("btop").unwrap();
        assert!(btop.bool_field("enable", false));
        assert_eq!(btop.get("move").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn an_override_changes_one_field_and_keeps_the_rest() {
        let base = default_toggles();
        let over = json::obj([(
            "music",
            json::obj([("spotify", json::obj([("enable", Json::Bool(false))]))]),
        )]);
        let merged = deep_merge(&base, &over);

        let spotify = merged.get("music").unwrap().get("spotify").unwrap();
        assert_eq!(spotify.get("enable").unwrap().as_bool(), Some(false), "overridden");
        assert!(spotify.get("command").is_some(), "the command survived the override");
        assert!(merged.get("sysmon").is_some(), "other workspaces untouched");
    }

    #[test]
    fn a_new_app_can_be_added_alongside_the_defaults() {
        let merged = deep_merge(
            &default_toggles(),
            &json::obj([(
                "music",
                json::obj([("mine", json::obj([("enable", Json::Bool(true))]))]),
            )]),
        );
        let music = merged.get("music").unwrap();
        assert!(music.get("mine").is_some());
        assert!(music.get("spotify").is_some());
    }
}

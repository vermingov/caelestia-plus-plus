//! `caelestia toggle <workspace>` — five of the keybinds in this config, and
//! the one pressed most often.
//!
//! A toggle is not only "show the special workspace": each workspace has apps
//! attached, which are started if they are not running and pulled onto the
//! workspace if they are. Only when nothing was started does the workspace
//! itself toggle, so the first press launches and the second reveals.

use redcommon::json::Json;

use crate::{config, hypr, proc};

pub fn run(workspace: &str) {
    if workspace == "specialws" {
        return toggle_focused_special();
    }

    let toggles = config::toggles();
    let mut session = Session::default();
    let mut spawned = false;

    if let Some(Json::Obj(apps)) = toggles.get(workspace) {
        for app in apps.values() {
            if app.bool_field("enable", false) && session.handle(app, workspace) {
                spawned = true;
            }
        }
    }

    // Starting an app already puts it on the workspace; toggling as well
    // would hide it again the moment it appeared.
    if !spawned {
        hypr::toggle_special_workspace(workspace);
    }
}

/// One run's view of the compositor. The client list is asked for at most
/// once however many apps a workspace has.
#[derive(Default)]
struct Session {
    clients: Option<Vec<Json>>,
}

impl Session {
    fn clients(&mut self) -> &[Json] {
        self.clients.get_or_insert_with(hypr::clients)
    }

    /// Returns whether this app was started.
    fn handle(&mut self, app: &Json, workspace: &str) -> bool {
        let Some(specs) = app.get("match") else { return false };

        let mut spawned = false;
        if let Some(Json::Arr(command)) = app.get("command") {
            let command: Vec<String> = command
                .iter()
                .filter_map(|p| p.as_str().map(str::to_string))
                .collect();
            spawned = self.spawn_if_absent(&command, specs, workspace);
        }
        if app.bool_field("move", false) {
            self.move_matching(specs, workspace);
        }
        spawned
    }

    fn spawn_if_absent(&mut self, command: &[String], specs: &Json, workspace: &str) -> bool {
        let Some(program) = command.first() else { return false };
        // A desktop file is launched by a handler that may well exist even
        // when the program behind it is not on PATH.
        if !(program.ends_with(".desktop") || proc::which(program)) {
            return false;
        }
        if self.clients().iter().any(|c| matches(c, specs)) {
            return false; // already running: the move below is what it needs
        }
        hypr::exec(&format!(
            "[workspace special:{workspace}] {}",
            proc::shell_join(command)
        ));
        true
    }

    fn move_matching(&mut self, specs: &Json, workspace: &str) {
        let target = format!("special:{workspace}");
        let moves: Vec<String> = self
            .clients()
            .iter()
            .filter(|c| matches(c, specs))
            .filter(|c| c.get("workspace").and_then(|w| w.str_field("name")) != Some(target.as_str()))
            .filter_map(|c| c.str_field("address").map(str::to_string))
            .collect();
        for address in moves {
            hypr::move_to_workspace_silent(&target, &address);
        }
    }
}

/// The workspace already showing on the focused monitor, toggled off — or the
/// default one toggled on when none is showing.
fn toggle_focused_special() {
    let Some(monitor) = hypr::focused_monitor() else { return };
    let showing = monitor
        .get("specialWorkspace")
        .and_then(|w| w.str_field("name"))
        .and_then(|name| name.strip_prefix("special:"))
        .filter(|name| !name.is_empty())
        .unwrap_or("special");
    hypr::toggle_special_workspace(showing);
}

/// One `match` entry is a set of fields that must all hold; the list of them
/// is alternatives. Mirrors the Python `is_subset`, substring rule included:
/// a class of "discord" is meant to match "discord-canary" as well.
fn matches(client: &Json, specs: &Json) -> bool {
    match specs {
        Json::Arr(alternatives) => alternatives.iter().any(|spec| is_subset(client, spec)),
        spec => is_subset(client, spec),
    }
}

fn is_subset(client: &Json, spec: &Json) -> bool {
    let Json::Obj(wanted) = spec else { return false };
    wanted.iter().all(|(key, want)| match client.get(key) {
        None => false,
        Some(have) => match want {
            Json::Obj(_) => is_subset(have, want),
            Json::Str(want) => have.as_str().is_some_and(|have| have.contains(want.as_str())),
            Json::Arr(wanted) => match have {
                Json::Arr(have) => wanted.iter().all(|w| have.contains(w)),
                _ => false,
            },
            want => have == want,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use redcommon::json;

    fn client(class: &str, title: &str, workspace: &str) -> Json {
        json::obj([
            ("class", json::s(class)),
            ("title", json::s(title)),
            ("address", json::s("0x1234")),
            ("workspace", json::obj([("name", json::s(workspace))])),
        ])
    }

    #[test]
    fn a_class_match_is_a_substring_like_the_daemon_it_replaces() {
        let spec = Json::Arr(vec![json::obj([("class", json::s("discord"))])]);
        assert!(matches(&client("discord", "Discord", "5"), &spec));
        assert!(matches(&client("discord-canary", "Discord", "5"), &spec), "a fork still counts");
        assert!(!matches(&client("Discord", "Discord", "5"), &spec), "and the case still matters");
        assert!(!matches(&client("firefox", "x", "5"), &spec));
    }

    #[test]
    fn every_field_of_one_alternative_must_hold() {
        let spec = Json::Arr(vec![json::obj([
            ("class", json::s("btop")),
            ("title", json::s("btop")),
            ("workspace", json::obj([("name", json::s("special:sysmon"))])),
        ])]);
        assert!(matches(&client("btop", "btop", "special:sysmon"), &spec));
        assert!(!matches(&client("btop", "btop", "3"), &spec), "wrong workspace");
        assert!(!matches(&client("btop", "htop", "special:sysmon"), &spec), "wrong title");
    }

    #[test]
    fn alternatives_are_alternatives() {
        let spec = Json::Arr(vec![
            json::obj([("class", json::s("Spotify"))]),
            json::obj([("initialTitle", json::s("Spotify Free"))]),
        ]);
        assert!(matches(&client("Spotify", "x", "2"), &spec));
        let by_title = json::obj([
            ("class", json::s("chrome")),
            ("initialTitle", json::s("Spotify Free")),
        ]);
        assert!(matches(&by_title, &spec));
        assert!(!matches(&client("chrome", "x", "2"), &spec));
    }

    #[test]
    fn a_missing_field_never_matches() {
        let spec = Json::Arr(vec![json::obj([("initialTitle", json::s("Spotify"))])]);
        assert!(!matches(&client("Spotify", "Spotify", "2"), &spec));
    }
}

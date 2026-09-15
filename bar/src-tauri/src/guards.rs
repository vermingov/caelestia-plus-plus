//! The firewall and the exploit guard, read from their own sockets.
//!
//! Both daemons speak the same contract: newline-delimited JSON over a Unix
//! socket, pushing `{"t":"rules",…}` and `{"t":"ask",…}` as things change.
//! The shell has its own client for this; the bar has one too rather than
//! asking the shell, because asking the shell means spawning `qs`, and `qs`
//! boots a QML runtime — which on a three-second tick was most of what this
//! whole process cost.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

/// Where each daemon listens. Both are root-owned sockets, group-readable by
/// the user the desktop runs as.
const SOCKETS: [(&str, &str); 2] =
    [("firewall", "/run/redwall/ui.sock"), ("protection", "/run/redguard/ui.sock")];

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Guards {
    pub connected: bool,
    /// Apps waiting on a verdict, plus anything the guard has frozen.
    pub pending: i64,
    pub rules: i64,
}

/// One daemon's picture of the world, plus the socket to answer it on.
#[derive(Default)]
struct Daemon {
    connected: bool,
    /// Whether the daemon is enforcing at all; rules are kept either way.
    enabled: bool,
    /// Applications waiting on a verdict right now.
    pending: Vec<Value>,
    /// Every remembered rule.
    rules: Vec<Value>,
    /// The write half, held so a verdict can be sent back down the same
    /// connection the prompt arrived on.
    socket: Option<UnixStream>,
}

/// One daemon's state, as the panel draws it.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub name: String,
    pub connected: bool,
    pub enabled: bool,
    pub pending: Vec<Value>,
    pub rules: Vec<Value>,
}

#[derive(Clone, Default)]
pub struct Watcher {
    daemons: Vec<(&'static str, Arc<Mutex<Daemon>>)>,
}

impl Watcher {
    /// Starts one reader thread per daemon. They reconnect on their own, so a
    /// daemon that is restarted comes back without the bar noticing.
    pub fn start() -> Watcher {
        let mut daemons = Vec::new();
        for (name, path) in SOCKETS {
            let shared = Arc::new(Mutex::new(Daemon::default()));
            daemons.push((name, Arc::clone(&shared)));
            std::thread::spawn(move || read_forever(name, path, shared));
        }
        Watcher { daemons }
    }

    /// The counts, which is all the bar's shield needs.
    pub fn read(&self) -> Guards {
        let mut guards = Guards::default();
        for (_, daemon) in &self.daemons {
            let Ok(daemon) = daemon.lock() else { continue };
            if daemon.connected {
                guards.connected = true;
                guards.pending += daemon.pending.len() as i64;
                guards.rules += daemon.rules.len() as i64;
            }
        }
        guards
    }

    /// Everything, which is what the panel draws.
    pub fn detail(&self) -> Vec<Detail> {
        self.daemons
            .iter()
            .filter_map(|(name, daemon)| {
                let daemon = daemon.lock().ok()?;
                Some(Detail {
                    name: name.to_string(),
                    connected: daemon.connected,
                    enabled: daemon.enabled,
                    pending: daemon.pending.clone(),
                    rules: daemon.rules.clone(),
                })
            })
            .collect()
    }

    /// Sends one message to one daemon, on the connection its prompts arrived
    /// on — a second connection would be a second client as far as it is
    /// concerned.
    fn send(&self, which: &str, message: Value) {
        let Some((_, daemon)) = self.daemons.iter().find(|(name, _)| *name == which) else { return };
        let Ok(mut daemon) = daemon.lock() else { return };
        let Some(socket) = daemon.socket.as_mut() else { return };
        let _ = socket.write_all(format!("{message}\n").as_bytes());
    }

    /// Answers one prompt. `remember` turns the answer into a standing rule.
    pub fn verdict(&self, which: &str, id: i64, action: &str, remember: bool) {
        if action != "allow" && action != "deny" {
            return;
        }
        self.send(which, json!({ "t": "verdict", "id": id, "action": action, "remember": remember }));
        // Taken out here as well as by the daemon's own reply, so the prompt
        // leaves the panel the moment it is answered.
        if let Some((_, daemon)) = self.daemons.iter().find(|(name, _)| *name == which) {
            if let Ok(mut daemon) = daemon.lock() {
                daemon.pending.retain(|prompt| prompt.get("id").and_then(Value::as_i64) != Some(id));
            }
        }
    }

    pub fn set_rule(&self, which: &str, exe: &str, action: &str, name: &str) {
        if action != "allow" && action != "deny" {
            return;
        }
        self.send(which, json!({ "t": "setrule", "exe": exe, "action": action, "name": name }));
    }

    pub fn delete_rule(&self, which: &str, exe: &str) {
        self.send(which, json!({ "t": "delrule", "exe": exe }));
    }

    pub fn set_enabled(&self, which: &str, enabled: bool) {
        self.send(which, json!({ "t": "setenabled", "enabled": enabled }));
    }
}

fn read_forever(name: &'static str, path: &'static str, shared: Arc<Mutex<Daemon>>) {
    loop {
        let Ok(mut socket) = UnixStream::connect(path) else {
            if let Ok(mut daemon) = shared.lock() {
                daemon.connected = false;
            }
            std::thread::sleep(Duration::from_secs(5));
            continue;
        };

        // The daemon pushes as things change, but says nothing at all until
        // something does — so the current picture has to be asked for.
        let _ = socket.write_all(b"{\"t\":\"getrules\"}\n");

        let reader = match socket.try_clone() {
            Ok(clone) => BufReader::new(clone),
            Err(_) => continue,
        };
        if let Ok(mut daemon) = shared.lock() {
            daemon.connected = true;
            daemon.enabled = true;
            daemon.socket = Some(socket);
        }

        for line in reader.lines().map_while(Result::ok) {
            if let Ok(mut daemon) = shared.lock() {
                apply(&line, &mut daemon);
            }
        }

        // The socket closed: the daemon stopped, or is being restarted.
        if let Ok(mut daemon) = shared.lock() {
            daemon.connected = false;
            daemon.pending.clear();
            daemon.socket = None;
        }
        eprintln!("caelestia-bar: {name} daemon disconnected, reconnecting");
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Folds one message into the counts.
///
/// The message vocabulary is the daemons', and it grows: anything unrecognised
/// is ignored rather than guessed at, so a new message type cannot make the
/// shield say something untrue.
fn apply(line: &str, daemon: &mut Daemon) {
    let Ok(message) = serde_json::from_str::<Value>(line) else { return };
    match message.get("t").and_then(Value::as_str) {
        // The full rule set, which arrives on connect and on every change.
        Some("rules") => {
            if let Some(rules) = message.get("rules").and_then(Value::as_array) {
                daemon.rules = rules.clone();
            }
            if let Some(pending) = message.get("pending").and_then(Value::as_array) {
                daemon.pending = pending.clone();
            }
            // A prompt whose application has just gained a rule — from here or
            // from anywhere else — has been answered.
            let ruled: Vec<&str> =
                daemon.rules.iter().filter_map(|rule| rule.get("exe").and_then(Value::as_str)).collect();
            let ruled: Vec<String> = ruled.into_iter().map(str::to_string).collect();
            daemon.pending.retain(|prompt| {
                prompt
                    .get("exe")
                    .and_then(Value::as_str)
                    .map(|exe| !ruled.iter().any(|known| known == exe))
                    .unwrap_or(true)
            });
        }
        // One application asking to be allowed through.
        Some("ask") => {
            let id = message.get("id").and_then(Value::as_i64);
            if !daemon.pending.iter().any(|prompt| prompt.get("id").and_then(Value::as_i64) == id) {
                daemon.pending.push(message);
            }
        }
        // Answered, by this bar or by anything else.
        Some("resolved") => {
            let exe = message.get("exe").and_then(Value::as_str).unwrap_or_default().to_string();
            daemon.pending.retain(|prompt| {
                prompt.get("exe").and_then(Value::as_str).unwrap_or_default() != exe
            });
        }
        // The master switch, which the daemon owns and persists.
        Some("state") => {
            daemon.enabled = message.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rules_message_sets_both_lists() {
        let mut daemon = Daemon::default();
        apply(
            r#"{"t":"rules","rules":[{"exe":"/usr/bin/curl"}],"pending":[{"id":1,"exe":"/usr/bin/wget"}]}"#,
            &mut daemon,
        );
        assert_eq!(daemon.rules.len(), 1);
        assert_eq!(daemon.pending.len(), 1);
    }

    #[test]
    fn a_prompt_whose_app_gains_a_rule_is_answered() {
        // The rule may have been added from the shell's panel or by another
        // client; either way the prompt is stale and must not stay up.
        let mut daemon = Daemon::default();
        apply(r#"{"t":"ask","id":1,"exe":"/usr/bin/curl"}"#, &mut daemon);
        assert_eq!(daemon.pending.len(), 1);

        apply(r#"{"t":"rules","rules":[{"exe":"/usr/bin/curl"}]}"#, &mut daemon);
        assert!(daemon.pending.is_empty());
    }

    #[test]
    fn the_same_prompt_twice_is_one_prompt() {
        let mut daemon = Daemon::default();
        apply(r#"{"t":"ask","id":4,"exe":"/usr/bin/ssh"}"#, &mut daemon);
        apply(r#"{"t":"ask","id":4,"exe":"/usr/bin/ssh"}"#, &mut daemon);
        assert_eq!(daemon.pending.len(), 1);
    }

    #[test]
    fn a_resolution_clears_that_application() {
        let mut daemon = Daemon::default();
        apply(r#"{"t":"ask","id":1,"exe":"/usr/bin/curl"}"#, &mut daemon);
        apply(r#"{"t":"ask","id":2,"exe":"/usr/bin/wget"}"#, &mut daemon);
        apply(r#"{"t":"resolved","exe":"/usr/bin/curl"}"#, &mut daemon);

        assert_eq!(daemon.pending.len(), 1);
        assert_eq!(daemon.pending[0]["exe"], "/usr/bin/wget");
    }

    #[test]
    fn the_master_switch_is_the_daemons_to_report() {
        let mut daemon = Daemon::default();
        apply(r#"{"t":"state","enabled":false}"#, &mut daemon);
        assert!(!daemon.enabled);
    }

    #[test]
    fn an_unknown_message_changes_nothing() {
        let mut daemon = Daemon::default();
        daemon.rules = vec![json!({"exe": "/usr/bin/curl"})];
        apply(r#"{"t":"somethingnew","whatever":true}"#, &mut daemon);
        apply("not json at all", &mut daemon);
        assert_eq!(daemon.rules.len(), 1);
    }
}

//! Persistent per-executable verdicts, and the enabled flag beside them.
//!
//! Same on-disk shape the Python daemons wrote, so an existing
//! `rules.json` / `state.json` carries over untouched:
//!   {"<exe>": {"action": "<word>", "name": "...", "added": <unix secs>}}
//!
//! The two daemons disagree about the words — redwall persists allow/deny,
//! redguard allow/block — so the store is generic over the vocabulary and each
//! daemon keeps its own enum. The file stays a plain string either way.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::json::{self, Json};
use crate::warn;

/// A daemon's answer to "what do we do about this executable", as stored.
pub trait Verdict: Copy + Eq + 'static {
    /// The word written to the rule file. Must round-trip through `parse`.
    fn as_str(self) -> &'static str;
    fn parse(word: &str) -> Option<Self>;
}

#[derive(Debug, Clone)]
pub struct Rule<V> {
    pub action: V,
    pub name: String,
    pub added: u64,
}

pub struct Rules<V: Verdict> {
    path: PathBuf,
    data: BTreeMap<String, Rule<V>>,
}

impl<V: Verdict> Rules<V> {
    pub fn load(path: impl Into<PathBuf>) -> Rules<V> {
        let path = path.into();
        let mut data = BTreeMap::new();

        if let Ok(text) = fs::read_to_string(&path) {
            match json::parse(&text) {
                Some(Json::Obj(map)) => {
                    for (exe, v) in map {
                        let Some(action) = v.str_field("action").and_then(V::parse) else {
                            // A word this daemon does not know is not a verdict
                            // it can enforce. Dropping the entry means the app
                            // is asked about again, which is the same thing the
                            // Python daemons did by falling through their
                            // equality checks.
                            warn!("ignoring rule for {exe}: unknown action");
                            continue;
                        };
                        let name = v
                            .str_field("name")
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| crate::procfs::file_name(&exe))
                            .to_string();
                        let added = v.get("added").and_then(Json::as_u64).unwrap_or(0);
                        data.insert(exe, Rule { action, name, added });
                    }
                }
                // A corrupt rule file must not wedge the daemon into a state
                // where nothing is remembered and every app re-prompts forever;
                // start empty and say so.
                _ => warn!("{} is not valid JSON, starting with no rules", path.display()),
            }
        }

        Rules { path, data }
    }

    pub fn action_for(&self, exe: &str) -> Option<V> {
        self.data.get(exe).map(|r| r.action)
    }

    pub fn contains(&self, exe: &str) -> bool {
        self.data.contains_key(exe)
    }

    pub fn set(&mut self, exe: &str, action: V, name: Option<&str>) {
        let entry = self.data.entry(exe.to_string()).or_insert_with(|| Rule {
            action,
            name: crate::procfs::file_name(exe).to_string(),
            added: now_secs(),
        });
        entry.action = action;
        if let Some(n) = name.filter(|n| !n.is_empty()) {
            entry.name = n.to_string();
        }
        self.save();
    }

    pub fn delete(&mut self, exe: &str) {
        if self.data.remove(exe).is_some() {
            self.save();
        }
    }

    /// Drop every rule whose executable the daemon considers unsafe to
    /// remember, and report what went. Rule files outlive the code that wrote
    /// them, so a rule that is poison today may well be sitting in one.
    pub fn purge(&mut self, unwanted: impl Fn(&str) -> bool) -> Vec<String> {
        let doomed: Vec<String> = self
            .data
            .keys()
            .filter(|exe| unwanted(exe))
            .cloned()
            .collect();
        for exe in &doomed {
            self.data.remove(exe);
        }
        if !doomed.is_empty() {
            self.save();
        }
        doomed
    }

    /// The rule list as the UI wants it: one flat object per rule.
    pub fn snapshot(&self) -> Json {
        Json::Arr(
            self.data
                .iter()
                .map(|(exe, r)| {
                    json::obj([
                        ("exe", json::s(exe.clone())),
                        ("action", json::s(r.action.as_str())),
                        ("name", json::s(r.name.clone())),
                        ("added", json::n(r.added as f64)),
                    ])
                })
                .collect(),
        )
    }

    fn save(&self) {
        let mut map = BTreeMap::new();
        for (exe, r) in &self.data {
            map.insert(
                exe.clone(),
                json::obj([
                    ("action", json::s(r.action.as_str())),
                    ("name", json::s(r.name.clone())),
                    ("added", json::n(r.added as f64)),
                ]),
            );
        }
        if let Err(e) = write_atomic(&self.path, Json::Obj(map).dump().as_bytes()) {
            warn!("rule save failed: {e}");
        }
    }
}

/// The enabled flag, kept beside the rules. Turning a daemon off passes
/// everything but keeps the rules, so the flag has to survive a restart.
pub struct State {
    path: PathBuf,
    pub enabled: bool,
}

impl State {
    pub fn load(path: impl Into<PathBuf>) -> State {
        let path = path.into();
        let enabled = fs::read_to_string(&path)
            .ok()
            .and_then(|t| json::parse(&t))
            .map(|v| v.bool_field("enabled", true))
            .unwrap_or(true);
        State { path, enabled }
    }

    pub fn set(&mut self, enabled: bool) {
        self.enabled = enabled;
        let body = json::obj([("enabled", Json::Bool(enabled))]).dump();
        if let Err(e) = write_atomic(&self.path, body.as_bytes()) {
            warn!("state save failed: {e}");
        }
    }
}

/// Write-then-rename, so a crash or a full disk cannot leave a half-written
/// rule file that reads back as "no rules at all".
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for a daemon's own vocabulary.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Act {
        Allow,
        Deny,
    }

    impl Verdict for Act {
        fn as_str(self) -> &'static str {
            match self {
                Act::Allow => "allow",
                Act::Deny => "deny",
            }
        }
        fn parse(word: &str) -> Option<Act> {
            match word {
                "allow" => Some(Act::Allow),
                "deny" => Some(Act::Deny),
                _ => None,
            }
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("redcommon-rules-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn round_trips_through_the_file() {
        let path = scratch("rules.json");
        let mut r: Rules<Act> = Rules::load(&path);
        assert!(r.action_for("/usr/bin/curl").is_none());
        r.set("/usr/bin/curl", Act::Allow, Some("curl"));
        r.set("/usr/bin/nc", Act::Deny, None);

        let reloaded: Rules<Act> = Rules::load(&path);
        assert_eq!(reloaded.action_for("/usr/bin/curl"), Some(Act::Allow));
        assert_eq!(reloaded.action_for("/usr/bin/nc"), Some(Act::Deny));
        // A missing name falls back to the basename rather than an empty label
        assert_eq!(reloaded.data["/usr/bin/nc"].name, "nc");
    }

    #[test]
    fn reads_the_python_daemons_format() {
        let path = scratch("legacy.json");
        fs::write(
            &path,
            r#"{"/opt/app/bin/thing":{"action":"deny","name":"thing","added":1700000000}}"#,
        )
        .unwrap();
        let r: Rules<Act> = Rules::load(&path);
        assert_eq!(r.action_for("/opt/app/bin/thing"), Some(Act::Deny));
        assert_eq!(r.data["/opt/app/bin/thing"].added, 1700000000);
    }

    #[test]
    fn keeps_the_added_timestamp_when_a_verdict_changes() {
        let path = scratch("added.json");
        fs::write(
            &path,
            r#"{"/usr/bin/x":{"action":"allow","name":"x","added":1700000000}}"#,
        )
        .unwrap();
        let mut r: Rules<Act> = Rules::load(&path);
        r.set("/usr/bin/x", Act::Deny, None);
        let reloaded: Rules<Act> = Rules::load(&path);
        assert_eq!(reloaded.action_for("/usr/bin/x"), Some(Act::Deny));
        assert_eq!(reloaded.data["/usr/bin/x"].added, 1700000000, "first seen, not last changed");
    }

    #[test]
    fn a_word_this_daemon_does_not_know_is_not_enforced() {
        let path = scratch("foreign.json");
        fs::write(
            &path,
            r#"{"/usr/bin/a":{"action":"block"},"/usr/bin/b":{"action":"allow"}}"#,
        )
        .unwrap();
        let r: Rules<Act> = Rules::load(&path);
        assert!(r.action_for("/usr/bin/a").is_none(), "unknown word means ask again");
        assert_eq!(r.action_for("/usr/bin/b"), Some(Act::Allow));
    }

    #[test]
    fn a_corrupt_file_does_not_panic() {
        let path = scratch("corrupt.json");
        fs::write(&path, "{ this is not json").unwrap();
        let r: Rules<Act> = Rules::load(&path);
        assert!(r.data.is_empty());
    }

    #[test]
    fn purge_drops_the_named_rules_and_persists() {
        let path = scratch("purge.json");
        let mut r: Rules<Act> = Rules::load(&path);
        r.set("/usr/bin/bash", Act::Deny, None);
        r.set("/opt/thing/thing", Act::Allow, None);

        let dropped = r.purge(|exe| crate::procfs::file_name(exe) == "bash");
        assert_eq!(dropped, vec!["/usr/bin/bash".to_string()]);

        let reloaded: Rules<Act> = Rules::load(&path);
        assert!(!reloaded.contains("/usr/bin/bash"), "gone from disk too");
        assert!(reloaded.contains("/opt/thing/thing"));
    }

    #[test]
    fn snapshot_carries_every_field_the_ui_shows() {
        let path = scratch("snap.json");
        let mut r: Rules<Act> = Rules::load(&path);
        r.set("/usr/bin/curl", Act::Allow, Some("curl"));
        let dump = r.snapshot().dump();
        for field in ["\"exe\"", "\"action\"", "\"name\"", "\"added\"", "allow", "curl"] {
            assert!(dump.contains(field), "{field} missing from {dump}");
        }
    }

    #[test]
    fn enabled_state_persists() {
        let path = scratch("state.json");
        assert!(State::load(&path).enabled, "defaults to on");
        State::load(&path).set(false);
        assert!(!State::load(&path).enabled);
    }
}

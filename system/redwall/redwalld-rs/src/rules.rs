//! Persistent per-executable verdicts, and the enabled flag.
//!
//! Same on-disk shape as the Python daemon wrote, so an existing
//! `rules.json` / `state.json` carries over untouched:
//!   {"<exe>": {"action": "allow"|"deny", "name": "...", "added": <unix secs>}}

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::json::{self, Json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Allow,
    Deny,
}

impl Action {
    pub fn parse(s: &str) -> Action {
        if s == "allow" {
            Action::Allow
        } else {
            Action::Deny
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Allow => "allow",
            Action::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub action: Action,
    pub name: String,
    pub added: u64,
}

pub struct Rules {
    path: PathBuf,
    data: BTreeMap<String, Rule>,
}

impl Rules {
    pub fn load(path: impl Into<PathBuf>) -> Rules {
        let path = path.into();
        let mut data = BTreeMap::new();

        if let Ok(text) = fs::read_to_string(&path) {
            if let Some(Json::Obj(map)) = json::parse(&text) {
                for (exe, v) in map {
                    let action = Action::parse(v.str_field("action").unwrap_or("deny"));
                    let name = v
                        .str_field("name")
                        .unwrap_or_else(|| exe.rsplit('/').next().unwrap_or(&exe))
                        .to_string();
                    let added = v.get("added").and_then(Json::as_u64).unwrap_or(0);
                    data.insert(exe, Rule { action, name, added });
                }
            } else {
                // A corrupt rule file must not wedge the firewall into a state
                // where nothing is remembered and every app re-prompts forever;
                // start empty and say so.
                eprintln!("[redwall] {} is not valid JSON, starting with no rules", path.display());
            }
        }

        Rules { path, data }
    }

    pub fn action_for(&self, exe: &str) -> Option<&Action> {
        self.data.get(exe).map(|r| &r.action)
    }

    pub fn set(&mut self, exe: &str, action: Action, name: Option<&str>) {
        let fallback = exe.rsplit('/').next().unwrap_or(exe).to_string();
        let entry = self.data.entry(exe.to_string()).or_insert(Rule {
            action: action.clone(),
            name: fallback,
            added: now_secs(),
        });
        entry.action = action;
        if let Some(n) = name {
            if !n.is_empty() {
                entry.name = n.to_string();
            }
        }
        self.save();
    }

    pub fn delete(&mut self, exe: &str) {
        if self.data.remove(exe).is_some() {
            self.save();
        }
    }

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
            eprintln!("[redwall] rule save failed: {e}");
        }
    }
}

/// The enabled flag, kept beside the rules. Disabling passes traffic but keeps
/// the rules, so it has to survive a restart.
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
            eprintln!("[redwall] state save failed: {e}");
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

    fn tmpdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("redwall-test-{}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn round_trips_through_the_file() {
        let path = tmpdir().join("rules.json");
        let _ = fs::remove_file(&path);

        let mut r = Rules::load(&path);
        assert!(r.action_for("/usr/bin/curl").is_none());
        r.set("/usr/bin/curl", Action::Allow, Some("curl"));
        r.set("/usr/bin/nc", Action::Deny, None);

        let reloaded = Rules::load(&path);
        assert_eq!(reloaded.action_for("/usr/bin/curl"), Some(&Action::Allow));
        assert_eq!(reloaded.action_for("/usr/bin/nc"), Some(&Action::Deny));
        // A missing name falls back to the basename rather than an empty label
        assert_eq!(reloaded.data["/usr/bin/nc"].name, "nc");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reads_the_python_daemons_format() {
        let path = tmpdir().join("legacy.json");
        fs::write(
            &path,
            r#"{"/opt/app/bin/thing":{"action":"deny","name":"thing","added":1700000000}}"#,
        )
        .unwrap();
        let r = Rules::load(&path);
        assert_eq!(r.action_for("/opt/app/bin/thing"), Some(&Action::Deny));
        assert_eq!(r.data["/opt/app/bin/thing"].added, 1700000000);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_corrupt_file_does_not_panic() {
        let path = tmpdir().join("corrupt.json");
        fs::write(&path, "{ this is not json").unwrap();
        let r = Rules::load(&path);
        assert!(r.snapshot().dump().contains("[]") || r.data.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn enabled_state_persists() {
        let path = tmpdir().join("state.json");
        let _ = fs::remove_file(&path);
        assert!(State::load(&path).enabled, "defaults to on");
        State::load(&path).set(false);
        assert!(!State::load(&path).enabled);
        let _ = fs::remove_file(&path);
    }
}

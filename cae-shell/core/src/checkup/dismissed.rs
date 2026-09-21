//! What has already been said once and does not need saying again.
//!
//! The startup prompt is for things the machine needs done to itself: a
//! package the shell wants and has not got, a privileged half left behind by
//! an update. Saying "later" to one of those means later for *that* one — a
//! newer version of the same half, or a different package, is a new thing
//! and is worth asking about again.
//!
//! Kept in the same file the QML shell kept it in, with the same shape, so
//! that whichever of the two is running reads what the other wrote.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Value, json};

/// Where it is kept.
fn file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let state = std::env::var_os("XDG_STATE_HOME").map_or_else(|| PathBuf::from(home).join(".local/state"), PathBuf::from);
    Some(state.join("caelestia/systemcheck.json"))
}

/// What has been waved away.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dismissed {
    /// Packages, by name.
    pub packages: Vec<String>,
    /// Privileged halves, by directory, with the version that was waved
    /// away: a newer one asks again.
    pub halves: HashMap<String, i64>,
}

impl Dismissed {
    pub fn read() -> Dismissed {
        let text = file().and_then(|file| std::fs::read_to_string(file).ok()).unwrap_or_default();
        Dismissed::said_by(&serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    fn said_by(saved: &Value) -> Dismissed {
        let packages = saved
            .get("dismissedPackages")
            .and_then(Value::as_array)
            .map(|listed| listed.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        let halves = saved
            .get("dismissedRootHalves")
            .and_then(Value::as_object)
            .map(|kept| kept.iter().filter_map(|(dir, at)| Some((dir.clone(), at.as_i64()?))).collect())
            .unwrap_or_default();
        Dismissed { packages, halves }
    }

    fn written(&self) -> Value {
        let halves: serde_json::Map<String, Value> = self.halves.iter().map(|(dir, at)| (dir.clone(), json!(at))).collect();
        json!({ "dismissedPackages": self.packages, "dismissedRootHalves": halves })
    }

    /// The ones that have not been waved away yet.
    pub fn fresh_packages(&self, missing: &[String]) -> Vec<String> {
        missing.iter().filter(|package| !self.packages.contains(package)).cloned().collect()
    }

    /// The halves whose version has moved since they were waved away.
    pub fn fresh_halves(&self, outdated: &[String], at: &HashMap<String, super::Version>) -> Vec<String> {
        outdated
            .iter()
            .filter(|dir| at.get(*dir).map_or(0, |version| version.repo) > self.halves.get(*dir).copied().unwrap_or(0))
            .cloned()
            .collect()
    }

    /// Waves away everything this scan found, and writes it down.
    pub fn wave_away(&mut self, missing: &[String], outdated: &[String], at: &HashMap<String, super::Version>) {
        for package in missing {
            if !self.packages.contains(package) {
                self.packages.push(package.clone());
            }
        }
        for dir in outdated {
            self.halves.insert(dir.clone(), at.get(dir).map_or(0, |version| version.repo));
        }
        let Some(path) = file() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(&self.written()).unwrap_or_default()));
    }
}

#[cfg(test)]
mod tests {
    use super::super::Version;
    use super::*;

    fn at(dir: &str, repo: i64) -> HashMap<String, Version> {
        HashMap::from([(dir.to_string(), Version { repo, installed: repo - 1, enabled: true })])
    }

    #[test]
    fn what_was_waved_away_stays_quiet() {
        let dismissed = Dismissed::said_by(&json!({ "dismissedPackages": ["swappy"] }));
        assert_eq!(dismissed.fresh_packages(&["swappy".to_string(), "ddcutil".to_string()]), vec!["ddcutil"]);
    }

    #[test]
    fn a_newer_version_of_a_waved_away_half_asks_again() {
        let dismissed = Dismissed::said_by(&json!({ "dismissedRootHalves": { "max-perf": 5 } }));
        let outdated = vec!["max-perf".to_string()];
        assert!(dismissed.fresh_halves(&outdated, &at("max-perf", 5)).is_empty(), "the same version stays quiet");
        assert_eq!(dismissed.fresh_halves(&outdated, &at("max-perf", 6)), vec!["max-perf"]);
    }

    #[test]
    fn nothing_saved_means_nothing_waved_away() {
        let dismissed = Dismissed::said_by(&json!(null));
        assert_eq!(dismissed, Dismissed::default());
        assert_eq!(dismissed.fresh_packages(&["swappy".to_string()]), vec!["swappy"]);
    }

    #[test]
    fn what_is_written_is_what_is_read_back() {
        let mut dismissed = Dismissed::default();
        dismissed.packages.push("swappy".to_string());
        dismissed.halves.insert("max-perf".to_string(), 7);
        assert_eq!(Dismissed::said_by(&dismissed.written()), dismissed);
    }
}

//! How often each application has been launched, and how recently.
//!
//! A launcher that lists alphabetically makes you type the same four letters
//! every day. This is the smallest thing that fixes it: a count per app that
//! decays, so what you used this week outranks what you used constantly a
//! year ago and have not touched since.

use std::collections::HashMap;
use std::path::PathBuf;

/// Launches older than this contribute nothing. Six weeks is long enough to
/// survive a holiday and short enough that a tool you have moved on from
/// stops jumping to the top.
const HALF_LIFE_DAYS: f64 = 14.0;
const MAX_AGE_DAYS: f64 = 42.0;

pub struct Usage {
    /// App id to the times it was launched, as unix seconds.
    launches: HashMap<String, Vec<u64>>,
    path: Option<PathBuf>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Usage {
    #[cfg(test)]
    pub fn empty() -> Usage {
        Usage { launches: HashMap::new(), path: None }
    }

    pub fn load() -> Usage {
        let path = state_path();
        let mut usage = Usage { launches: HashMap::new(), path: Some(path.clone()) };
        let Ok(text) = std::fs::read_to_string(&path) else { return usage };
        let Ok(parsed) = serde_json::from_str::<HashMap<String, Vec<u64>>>(&text) else {
            return usage;
        };
        usage.launches = parsed;
        usage.forget_old();
        usage
    }

    /// The clock, as the scorer wants it. Ranking reads it once and passes it
    /// to every `score_at`, rather than each call making its own syscall.
    pub fn now_secs(&self) -> f64 {
        now() as f64
    }

    /// A launch is worth 100 when it happens and halves every fortnight, so a
    /// handful of recent launches outweighs a pile of stale ones.
    #[cfg(test)]
    pub fn score(&self, id: &str) -> i64 {
        self.score_at(id, self.now_secs())
    }

    /// As `score`, against a clock the caller already read.
    pub fn score_at(&self, id: &str, now: f64) -> i64 {
        let Some(times) = self.launches.get(id) else { return 0 };
        let total: f64 = times
            .iter()
            .map(|at| {
                let age_days = (now - *at as f64).max(0.0) / 86_400.0;
                100.0 * 0.5f64.powf(age_days / HALF_LIFE_DAYS)
            })
            .sum();
        total as i64
    }

    pub fn record(&mut self, id: &str) {
        self.launches.entry(id.to_string()).or_default().push(now());
        self.forget_old();
        self.save();
    }

    fn forget_old(&mut self) {
        let cutoff = now().saturating_sub((MAX_AGE_DAYS * 86_400.0) as u64);
        for times in self.launches.values_mut() {
            times.retain(|at| *at >= cutoff);
        }
        self.launches.retain(|_, times| !times.is_empty());
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string(&self.launches) {
            let _ = std::fs::write(path, text);
        }
    }
}

fn state_path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"));
    state.join("caelestia/launcher-usage.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recent_launch_outscores_an_old_one() {
        let mut usage = Usage::empty();
        let now = now();
        usage.launches.insert("fresh".into(), vec![now]);
        usage.launches.insert("stale".into(), vec![now - (30 * 86_400)]);
        assert!(usage.score("fresh") > usage.score("stale"));
        assert_eq!(usage.score("never"), 0);
    }

    #[test]
    fn launches_past_the_window_are_forgotten() {
        let mut usage = Usage::empty();
        usage.launches.insert("ancient".into(), vec![now() - (90 * 86_400)]);
        usage.forget_old();
        assert!(!usage.launches.contains_key("ancient"));
    }
}

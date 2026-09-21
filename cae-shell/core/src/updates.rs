//! Whether a newer Caelestia++ has been published.
//!
//! Releases, not commits: the checkout only ever moves to a tag somebody
//! published on purpose. Read with git rather than from a web API, so there
//! is no rate limit and no token. Moving the checkout is the updater's job,
//! `cae`, which also rebuilds what the release needs and asks for root once
//! if it needs that; this only looks.

use crate::about::git;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// The newest release there is. Empty when nothing has been published.
    pub release: String,
    /// How many commits the checkout is behind it, which is none when it is
    /// on it or ahead of it.
    pub behind: usize,
    /// What those commits are, newest first, by their subject lines.
    pub changes: Vec<String>,
}

/// Fetches the tags and compares. An error is the network's, nearly always.
pub fn check() -> Result<Found, String> {
    git(&["fetch", "--quiet", "--tags", "--prune", "--prune-tags", "origin"]).ok_or("Could not reach the update server")?;
    let tags = git(&["tag", "--list", "v*", "--sort=-v:refname"]).unwrap_or_default();
    let Some(release) = tags.lines().next().map(str::to_string) else { return Ok(Found::default()) };

    let range = format!("HEAD..{release}^{{commit}}");
    let behind = git(&["rev-list", "--count", &range]).and_then(|count| count.parse().ok()).unwrap_or(0);
    let changes = git(&["log", "--format=%s", &range]).unwrap_or_default().lines().map(str::to_string).collect();
    Ok(Found { release, behind, changes })
}

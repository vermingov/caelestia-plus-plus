//! Ranking the app list against what has been typed.
//!
//! A launcher is judged on whether the thing you meant is first, so the score
//! is deliberately opinionated: a prefix of the name beats a word boundary
//! inside it, which beats a subsequence scattered through it, and how often
//! you have actually launched something breaks the ties.

use super::apps::App;
use super::usage::Usage;

/// Where a match landed, worst to best. The variants order the way the
/// results should.
const SCORE_SUBSEQUENCE: i64 = 100;
const SCORE_CONTAINS: i64 = 300;
const SCORE_WORD_START: i64 = 600;
const SCORE_PREFIX: i64 = 1000;
const SCORE_EXACT: i64 = 1400;

/// Matches `needle` against `haystack` as a scattered subsequence, and scores
/// how tight the run was — "fox" against "firefox" should beat "fox" against
/// "f-o-something-x".
fn subsequence_score(haystack: &str, needle: &str) -> Option<i64> {
    let mut chars = haystack.char_indices();
    let mut last = None;
    let mut gaps = 0i64;

    for wanted in needle.chars() {
        let found = chars.find(|(_, c)| *c == wanted)?;
        if let Some(previous) = last {
            gaps += (found.0 as i64 - previous as i64 - 1).max(0);
        }
        last = Some(found.0);
    }
    // A tight run scores near the full subsequence value; a scattered one
    // trails off but still ranks above no match at all.
    Some(SCORE_SUBSEQUENCE - gaps.min(SCORE_SUBSEQUENCE - 1))
}

/// How well one field matches, or None if it does not.
fn field_score(field: &str, needle: &str) -> Option<i64> {
    if field == needle {
        return Some(SCORE_EXACT);
    }
    if field.starts_with(needle) {
        // Longer prefixes of short names are better matches than the same
        // prefix of a long one: "term" should find "Terminal" before
        // "Terminal Emulator Settings".
        return Some(SCORE_PREFIX - field.len() as i64);
    }
    if let Some(at) = field.find(needle) {
        let after_boundary = field[..at].ends_with([' ', '-', '_', '.']);
        let base = if after_boundary { SCORE_WORD_START } else { SCORE_CONTAINS };
        return Some(base - at as i64);
    }
    subsequence_score(field, needle)
}

pub fn rank<'a>(apps: &'a [App], query: &str, usage: &Usage) -> Vec<&'a App> {
    let needle = query.trim().to_lowercase();
    // Decayed usage is a function of the time of the call, so the clock is
    // read once here rather than per comparison: a sort calls its comparator
    // n log n times, and this used to do a syscall and a powf inside each.
    let now = usage.now_secs();

    // Nothing typed: the list is whatever has been launched most, then
    // everything else alphabetically.
    if needle.is_empty() {
        let mut ordered: Vec<(i64, &App)> =
            apps.iter().map(|app| (usage.score_at(&app.id, now), app)).collect();
        ordered.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        return ordered.into_iter().map(|(_, app)| app).collect();
    }

    let mut scored: Vec<(i64, &App)> = apps
        .iter()
        .filter_map(|app| {
            // The name is what people search; a comment or keyword match is
            // real but should never outrank a name match.
            let score = field_score(&app.name_lower, &needle)
                .map(|s| s * 4)
                .or_else(|| field_score(&app.keywords, &needle).map(|s| s * 2))
                .or_else(|| field_score(&app.haystack, &needle))?;
            Some((score + usage.score_at(&app.id, now), app))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored.into_iter().map(|(_, app)| app).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> App {
        App {
            id: name.to_lowercase(),
            name: name.to_string(),
            comment: String::new(),
            icon: String::new(),
            exec: name.to_lowercase(),
            terminal: false,
            haystack: name.to_lowercase(),
            name_lower: name.to_lowercase(),
            keywords: String::new(),
        }
    }

    fn names<'a>(ranked: &[&'a App]) -> Vec<&'a str> {
        ranked.iter().map(|a| a.name.as_str()).collect()
    }

    /// Not a benchmark — a floor. Ranking runs on every keystroke with the
    /// UI thread waiting on it, so a change that makes it allocate per app
    /// again should fail here rather than be noticed as lag.
    #[test]
    fn ranking_a_large_list_stays_under_a_millisecond() {
        let apps: Vec<App> = (0..1000).map(|i| app(&format!("Application {i}"))).collect();
        let usage = Usage::empty();

        let started = std::time::Instant::now();
        for _ in 0..50 {
            std::hint::black_box(rank(&apps, "app", &usage));
        }
        let each = started.elapsed() / 50;

        assert!(each.as_micros() < 1000, "ranking 1000 apps took {each:?}");
    }

    #[test]
    fn a_prefix_beats_a_match_in_the_middle() {
        let apps = [app("Firefox"), app("Fire"), app("Wildfire")];
        let ranked = rank(&apps, "fire", &Usage::empty());
        assert_eq!(names(&ranked), ["Fire", "Firefox", "Wildfire"]);
    }

    #[test]
    fn a_word_start_beats_a_letter_soup_match() {
        let apps = [app("Visual Studio Code"), app("Vosk Decoder")];
        let ranked = rank(&apps, "code", &Usage::empty());
        assert_eq!(names(&ranked)[0], "Visual Studio Code");
    }

    #[test]
    fn an_initialism_still_finds_it() {
        let apps = [app("GNU Image Manipulation Program"), app("Nothing Here")];
        let ranked = rank(&apps, "gimp", &Usage::empty());
        assert_eq!(names(&ranked)[0], "GNU Image Manipulation Program");
    }

    #[test]
    fn what_you_launch_most_wins_a_tie() {
        let apps = [app("Terminal A"), app("Terminal B")];
        let mut usage = Usage::empty();
        for _ in 0..5 {
            usage.record("terminal b");
        }
        let ranked = rank(&apps, "terminal", &usage);
        assert_eq!(names(&ranked)[0], "Terminal B");
    }

    #[test]
    fn an_empty_query_lists_the_most_used_first() {
        let apps = [app("Alpha"), app("Beta"), app("Gamma")];
        let mut usage = Usage::empty();
        usage.record("gamma");
        let ranked = rank(&apps, "  ", &usage);
        assert_eq!(names(&ranked), ["Gamma", "Alpha", "Beta"]);
    }

    #[test]
    fn nothing_matches_nonsense() {
        let apps = [app("Firefox")];
        assert!(rank(&apps, "zzqqxx", &Usage::empty()).is_empty());
    }
}

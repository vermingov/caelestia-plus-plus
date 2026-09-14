//! Python's `difflib`, enough of it to suggest what a typo meant.
//!
//! The config doctor renames a misspelled setting to the closest real one, so
//! the closeness measure has to be the same one the script used or the two
//! would disagree about what the user meant — and a wrong rename silently
//! moves a setting somewhere it has no effect.
//!
//! This is `SequenceMatcher.ratio` and `get_close_matches` for short strings.
//! Nothing here handles the "autojunk" heuristic, which only applies to
//! sequences of 200 elements or more; a setting name is never that long.

use std::collections::HashMap;

/// How alike two strings are, as `2 * matched / total length`, 0 to 1.
///
/// Not symmetric: the matcher searches `a` for the longest run that also
/// appears in `b`, and which way round they go can change the answer. Python
/// passes the candidate as `a` and the misspelled word as `b`.
pub fn ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }

    // Where each character of b appears, which is what the search walks.
    let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
    for (j, ch) in b.iter().enumerate() {
        b2j.entry(*ch).or_default().push(j);
    }

    let matched = matching_size(&a, &b, &b2j, 0, a.len(), 0, b.len());
    2.0 * matched as f64 / total as f64
}

/// The total length of the matching blocks: take the longest run, then do the
/// same to what is left on either side of it.
fn matching_size(
    a: &[char],
    b: &[char],
    b2j: &HashMap<char, Vec<usize>>,
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
) -> usize {
    let (i, j, k) = longest_match(a, b, b2j, alo, ahi, blo, bhi);
    if k == 0 {
        return 0;
    }
    let mut total = k;
    if alo < i && blo < j {
        total += matching_size(a, b, b2j, alo, i, blo, j);
    }
    if i + k < ahi && j + k < bhi {
        total += matching_size(a, b, b2j, i + k, ahi, j + k, bhi);
    }
    total
}

/// The longest run shared by `a[alo..ahi]` and `b[blo..bhi]`, as (i, j, size).
/// Ties go to the earliest in `a`, then the earliest in `b`.
fn longest_match(
    a: &[char],
    b: &[char],
    b2j: &HashMap<char, Vec<usize>>,
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
) -> (usize, usize, usize) {
    let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);

    // j2len[j] is the length of the run ending at a[i] and b[j]; rebuilt for
    // each i from the previous row, which is what keeps this linear-ish.
    let mut j2len: HashMap<usize, usize> = HashMap::new();
    for i in alo..ahi {
        let mut newj2len: HashMap<usize, usize> = HashMap::new();
        if let Some(indices) = b2j.get(&a[i]) {
            for j in indices {
                let j = *j;
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = j.checked_sub(1).and_then(|prev| j2len.get(&prev).copied()).unwrap_or(0) + 1;
                newj2len.insert(j, k);
                if k > bestsize {
                    besti = i + 1 - k;
                    bestj = j + 1 - k;
                    bestsize = k;
                }
            }
        }
        j2len = newj2len;
    }

    // With no junk the best run can still be grown by whatever sits either
    // side of it, which the index search skipped.
    while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
        besti -= 1;
        bestj -= 1;
        bestsize += 1;
    }
    while besti + bestsize < ahi && bestj + bestsize < bhi && a[besti + bestsize] == b[bestj + bestsize] {
        bestsize += 1;
    }

    (besti, bestj, bestsize)
}

/// The candidate closest to `word`, or None when none is close enough.
///
/// Ties go to the candidate that sorts last, which is what Python's
/// `nlargest` on `(score, name)` pairs does.
pub fn closest_match<'a>(word: &str, candidates: impl IntoIterator<Item = &'a str>, cutoff: f64) -> Option<&'a str> {
    let mut best: Option<(f64, &str)> = None;
    for candidate in candidates {
        let score = ratio(candidate, word);
        if score < cutoff {
            continue;
        }
        let better = match best {
            None => true,
            Some((best_score, best_name)) => {
                score > best_score || (score == best_score && candidate > best_name)
            }
        };
        if better {
            best = Some((score, candidate));
        }
    }
    best.map(|(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_and_disjoint_are_the_extremes() {
        assert_eq!(ratio("abc", "abc"), 1.0);
        assert_eq!(ratio("", ""), 1.0);
        assert_eq!(ratio("abc", "xyz"), 0.0);
    }

    /// Every ratio and every lookup against Python's own difflib, over a
    /// corpus of setting names, typos of them, and random strings.
    #[test]
    fn matches_pythons_difflib() {
        use redcommon::json::Json;

        let vectors = redcommon::json::parse(include_str!("../tests/difflib-vectors.json"))
            .expect("reference vectors are readable");
        let Some(Json::Arr(pairs)) = vectors.get("pairs") else { panic!("no pairs") };
        let Some(Json::Arr(names)) = vectors.get("names") else { panic!("no names") };
        let Some(Json::Arr(closest)) = vectors.get("closest") else { panic!("no lookups") };

        let text = |v: &Json| match v {
            Json::Str(s) => s.clone(),
            _ => panic!("expected a string"),
        };

        for pair in pairs {
            let Json::Arr(pair) = pair else { panic!("triples") };
            let (a, b) = (text(&pair[0]), text(&pair[1]));
            let Json::Num(expected) = pair[2] else { panic!("a ratio") };
            let ours = ratio(&a, &b);
            assert!((ours - expected).abs() < 1e-12, "ratio({a:?}, {b:?}) = {ours} not {expected}");
        }

        let names: Vec<String> = names.iter().map(text).collect();
        for lookup in closest {
            let Json::Arr(lookup) = lookup else { panic!("pairs") };
            let word = text(&lookup[0]);
            let expected = match &lookup[1] {
                Json::Str(s) => Some(s.as_str()),
                _ => None,
            };
            let ours = closest_match(&word, names.iter().map(String::as_str), 0.6);
            assert_eq!(ours, expected, "closest to {word:?}");
        }
        assert!(pairs.len() > 3000, "corpus shrank");
    }

    #[test]
    fn a_typo_finds_its_setting() {
        let settings = ["background", "border", "rounding", "enabled", "visualiser"];
        assert_eq!(closest_match("backgroud", settings, 0.6), Some("background"));
        assert_eq!(closest_match("enabeld", settings, 0.6), Some("enabled"));
        assert_eq!(closest_match("visualizer", settings, 0.6), Some("visualiser"));
        assert_eq!(closest_match("completely-different", settings, 0.6), None);
    }

    #[test]
    fn a_tie_goes_to_the_later_name() {
        // Both score the same against "aa"; Python's nlargest picks the
        // larger of the two names.
        assert_eq!(closest_match("aa", ["aab", "aac"], 0.6), Some("aac"));
    }
}

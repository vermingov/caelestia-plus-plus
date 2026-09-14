//! Picking the one colour a scheme is built from.
//!
//! The quantiser hands back up to 128 colours with how much of the image each
//! covers. This ranks them: mostly by how much of the image shares their hue,
//! partly by how saturated they are, and then takes the most saturated one
//! that is not too dark to build a palette on.
//!
//! Ported from caelestia's `utils/material/score.py`, which is Material's
//! scorer with the filter left off.

use std::collections::BTreeMap;

use super::dislike;
use super::hct::Hct;
use super::math::{round_half_to_even, sanitize_degrees_int};

const TARGET_CHROMA: f64 = 48.0;
const WEIGHT_PROPORTION: f64 = 0.7;
const WEIGHT_CHROMA_ABOVE: f64 = 0.3;
const WEIGHT_CHROMA_BELOW: f64 = 0.1;
const CUTOFF_CHROMA: f64 = 5.0;
const CUTOFF_EXCITED_PROPORTION: f64 = 0.01;

/// What Material falls back to when an image has no colour worth seeding
/// from — a pure black or pure white wallpaper, say. The Python recurses
/// forever instead of answering; this is the answer its own upstream gives.
const FALLBACK: u32 = 0xff42_85f4;

pub fn score(colours_to_population: &BTreeMap<u32, u32>) -> Hct {
    pick(colours_to_population, false).unwrap_or_else(|| Hct::from_int(FALLBACK))
}

fn pick(colours_to_population: &BTreeMap<u32, u32>, filter_enabled: bool) -> Option<Hct> {
    let mut colours_hct = Vec::with_capacity(colours_to_population.len());
    let mut hue_population = [0u64; 360];
    let mut population_sum = 0u64;

    for (rgb, population) in colours_to_population {
        let hct = Hct::from_int(*rgb);
        // Truncated, not rounded — this bucket is only for the spread below.
        hue_population[hct.hue() as usize % 360] += *population as u64;
        population_sum += *population as u64;
        colours_hct.push(hct);
    }
    if population_sum == 0 {
        return None;
    }

    // Each hue lends its share to the 30 degrees around it, so a colour is
    // rewarded for the whole band it sits in rather than its exact hue.
    let mut hue_excited_proportions = [0.0f64; 360];
    for hue in 0..360i64 {
        let proportion = hue_population[hue as usize] as f64 / population_sum as f64;
        for i in hue - 14..hue + 16 {
            hue_excited_proportions[sanitize_degrees_int(i) as usize] += proportion;
        }
    }

    let mut scored: Vec<(Hct, f64)> = Vec::with_capacity(colours_hct.len());
    for hct in colours_hct {
        let hue = sanitize_degrees_int(round_half_to_even(hct.hue()) as i64) as usize;
        let proportion = hue_excited_proportions[hue];
        if filter_enabled && (hct.chroma() < CUTOFF_CHROMA || proportion <= CUTOFF_EXCITED_PROPORTION) {
            continue;
        }
        let proportion_score = proportion * 100.0 * WEIGHT_PROPORTION;
        let chroma_weight = if hct.chroma() < TARGET_CHROMA { WEIGHT_CHROMA_BELOW } else { WEIGHT_CHROMA_ABOVE };
        scored.push((hct, proportion_score + (hct.chroma() - TARGET_CHROMA) * chroma_weight));
    }

    // Stable, so colours that score the same keep the order the quantiser
    // returned them in — which is by ARGB, and is what Python compares equal.
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Relax the "colourful enough and light enough" bar until something
    // passes; a washed-out photo still has to yield a seed.
    for cutoff in (0..=20).rev() {
        let cutoff = cutoff as f64;
        if let Some((hct, _)) = scored.iter().find(|(h, _)| h.chroma() > cutoff && h.tone() > cutoff * 3.0) {
            return Some(dislike::fix_if_disliked(*hct));
        }
    }
    None
}

/// The whole wallpaper-to-seed chain: decode, quantise, score.
pub fn score_image(bytes: &[u8]) -> Result<Hct, String> {
    let image = super::jpeg::decode(bytes)?;
    let pixels = super::quantize::pixels_of(&image);
    Ok(score(&super::quantize::quantize_celebi(&pixels, 128)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use redcommon::json::Json;

    #[test]
    fn seeds_match_the_python_scorer() {
        let vectors = redcommon::json::parse(include_str!("../../tests/score-vectors.json"))
            .expect("reference vectors are readable");
        let Some(Json::Arr(images)) = vectors.get("images") else { panic!("no images") };

        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/jpeg");
        let mut checked = 0usize;
        for image in images {
            let name = image.str_field("name").expect("image name");
            let Some(expected) = image.get("seed").and_then(Json::as_u64) else {
                // Python raises RecursionError on these; we answer instead.
                continue;
            };
            let bytes = std::fs::read(format!("{dir}/{name}")).expect("test image is readable");
            let ours = score_image(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(ours.to_int() as u64, expected, "{name}: seed colour");
            checked += 1;
        }
        assert!(checked >= 40, "only {checked} images scored");
    }

    #[test]
    fn an_image_with_nothing_to_seed_from_still_answers() {
        // Pure black: no colour clears even the loosest cutoff. Python
        // recurses until it blows the stack.
        let mut only_black = BTreeMap::new();
        only_black.insert(0xff00_0000u32, 100u32);
        assert_eq!(score(&only_black).to_int(), FALLBACK);
    }
}

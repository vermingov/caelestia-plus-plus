//! The one region of the colour space people reliably dislike: dark, muddy
//! yellow-greens. Material moves them somewhere lighter rather than showing
//! them.
//!
//! Ported from materialyoucolor's `dislike/dislike_analyzer.py`.

use super::hct::Hct;
use super::math::round_half_to_even;

pub fn is_disliked(hct: &Hct) -> bool {
    let hue = round_half_to_even(hct.hue());
    (90.0..=111.0).contains(&hue)
        && round_half_to_even(hct.chroma()) > 16.0
        && round_half_to_even(hct.tone()) < 65.0
}

pub fn fix_if_disliked(hct: Hct) -> Hct {
    if is_disliked(&hct) {
        Hct::from_hct(hct.hue(), hct.chroma(), 70.0)
    } else {
        hct
    }
}

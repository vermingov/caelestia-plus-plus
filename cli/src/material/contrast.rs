//! Contrast between two tones, and the tone that reaches a given ratio.
//!
//! Ported from materialyoucolor's `contrast/contrast.py`. The 0.4 nudges and
//! the 0.04 tolerance are deliberate: they keep a tone that is nominally
//! exact from failing the ratio once it is rounded to a real colour.

use super::colour::{lstar_from_y, y_from_lstar};

pub fn ratio_of_tones(tone_a: f64, tone_b: f64) -> f64 {
    ratio_of_ys(
        y_from_lstar(tone_a.clamp(0.0, 100.0)),
        y_from_lstar(tone_b.clamp(0.0, 100.0)),
    )
}

pub fn ratio_of_ys(y1: f64, y2: f64) -> f64 {
    let lighter = y1.max(y2);
    let darker = if lighter == y2 { y1 } else { y2 };
    (lighter + 5.0) / (darker + 5.0)
}

/// The lighter tone that sits `ratio` above this one, or -1 when no tone does.
pub fn lighter(tone: f64, ratio: f64) -> f64 {
    if !(0.0..=100.0).contains(&tone) {
        return -1.0;
    }
    let dark_y = y_from_lstar(tone);
    let light_y = ratio * (dark_y + 5.0) - 5.0;
    let real = ratio_of_ys(light_y, dark_y);
    if real < ratio && (real - ratio).abs() > 0.04 {
        return -1.0;
    }
    let value = lstar_from_y(light_y) + 0.4;
    if !(0.0..=100.0).contains(&value) {
        return -1.0;
    }
    value
}

pub fn darker(tone: f64, ratio: f64) -> f64 {
    if !(0.0..=100.0).contains(&tone) {
        return -1.0;
    }
    let light_y = y_from_lstar(tone);
    let dark_y = ((light_y + 5.0) / ratio) - 5.0;
    let real = ratio_of_ys(light_y, dark_y);
    if real < ratio && (real - ratio).abs() > 0.04 {
        return -1.0;
    }
    let value = lstar_from_y(dark_y) - 0.4;
    if !(0.0..=100.0).contains(&value) {
        return -1.0;
    }
    value
}

/// As above, but white and black are acceptable answers when nothing else is.
pub fn lighter_unsafe(tone: f64, ratio: f64) -> f64 {
    let safe = lighter(tone, ratio);
    if safe < 0.0 {
        100.0
    } else {
        safe
    }
}

pub fn darker_unsafe(tone: f64, ratio: f64) -> f64 {
    let safe = darker(tone, ratio);
    if safe < 0.0 {
        0.0
    } else {
        safe
    }
}

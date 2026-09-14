//! Moving one colour towards another.
//!
//! Ported from materialyoucolor's `blend/blend.py`.

use super::cam16::Cam16;
use super::colour::lstar_from_argb;
use super::hct::Hct;
use super::math::{difference_degrees, rotation_direction, sanitize_degrees_double};

/// Rotate a colour's hue towards another's — at most 15 degrees, so it stays
/// recognisably itself while recognisably belonging to the scheme.
pub fn harmonize(design: u32, source: u32) -> u32 {
    let from = Hct::from_int(design);
    let to = Hct::from_int(source);
    let rotation = (difference_degrees(from.hue(), to.hue()) * 0.5).min(15.0);
    let hue = sanitize_degrees_double(
        from.hue() + rotation * rotation_direction(from.hue(), to.hue()),
    );
    Hct::from_hct(hue, from.chroma(), from.tone()).to_int()
}

/// Blend one hue into another, keeping the first colour's chroma and tone.
pub fn hct_hue(from: u32, to: u32, amount: f64) -> u32 {
    let blended = Cam16::from_int(cam16_ucs(from, to, amount));
    let original = Cam16::from_int(from);
    Hct::from_hct(blended.hue, original.chroma, lstar_from_argb(from)).to_int()
}

/// Straight interpolation in CAM16-UCS, where equal steps look equal.
pub fn cam16_ucs(from: u32, to: u32, amount: f64) -> u32 {
    let a = Cam16::from_int(from);
    let b = Cam16::from_int(to);
    Cam16::from_ucs(
        a.jstar + (b.jstar - a.jstar) * amount,
        a.astar + (b.astar - a.astar) * amount,
        a.bstar + (b.bstar - a.bstar) * amount,
    )
    .to_int()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::{contrast, dislike};
    use redcommon::json::Json;

    fn vectors() -> Json {
        redcommon::json::parse(include_str!("../../tests/blend-vectors.json"))
            .expect("reference vectors are readable")
    }

    fn cases(vectors: &Json, name: &str) -> Vec<Vec<f64>> {
        let Some(Json::Arr(rows)) = vectors.get(name) else {
            panic!("no {name} vectors")
        };
        rows.iter()
            .filter_map(|row| match row {
                Json::Arr(fields) => Some(
                    fields
                        .iter()
                        .map(|f| match f {
                            Json::Num(n) => *n,
                            _ => f64::NAN,
                        })
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn blending_matches_the_library() {
        let vectors = vectors();

        for case in cases(&vectors, "cam16_ucs") {
            let (from, to, amount, expected) =
                (case[0] as u32, case[1] as u32, case[2], case[3] as u32);
            assert_eq!(
                cam16_ucs(from, to, amount),
                expected,
                "cam16_ucs({from:#010x}, {to:#010x}, {amount})"
            );
        }
        for case in cases(&vectors, "harmonize") {
            let (design, source, expected) = (case[0] as u32, case[1] as u32, case[2] as u32);
            assert_eq!(
                harmonize(design, source),
                expected,
                "harmonize({design:#010x}, {source:#010x})"
            );
        }
        for case in cases(&vectors, "hct_hue") {
            let (from, to, amount, expected) =
                (case[0] as u32, case[1] as u32, case[2], case[3] as u32);
            assert_eq!(
                hct_hue(from, to, amount),
                expected,
                "hct_hue({from:#010x}, {to:#010x}, {amount})"
            );
        }
    }

    #[test]
    fn contrast_matches_the_library() {
        let vectors = vectors();

        for case in cases(&vectors, "ratio") {
            let got = contrast::ratio_of_tones(case[0], case[1]);
            assert!(
                (got - case[2]).abs() < 1e-12,
                "ratio_of_tones({}, {}): {got} vs {}",
                case[0],
                case[1],
                case[2]
            );
        }
        for (name, f) in [
            ("lighter", contrast::lighter as fn(f64, f64) -> f64),
            ("darker", contrast::darker as fn(f64, f64) -> f64),
        ] {
            for case in cases(&vectors, name) {
                let got = f(case[0], case[1]);
                assert!(
                    (got - case[2]).abs() < 1e-12,
                    "{name}({}, {}): {got} vs {}",
                    case[0],
                    case[1],
                    case[2]
                );
            }
        }
    }

    #[test]
    fn the_disliked_region_is_moved_the_same_way() {
        for case in cases(&vectors(), "dislike") {
            let (input, expected) = (case[0] as u32, case[1] as u32);
            let fixed = dislike::fix_if_disliked(crate::material::hct::Hct::from_int(input));
            assert_eq!(fixed.to_int(), expected, "fix_if_disliked({input:#010x})");
        }
    }
}

//! A colour as a hue, a chroma and a tone, and the sRGB value that carries
//! them. Setting one of the three re-solves the other two, because not every
//! combination exists.

use super::cam16::Cam16;
use super::colour::lstar_from_argb;
use super::solver::solve_to_int;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hct {
    hue: f64,
    chroma: f64,
    tone: f64,
    argb: u32,
}

impl Hct {
    pub fn from_int(argb: u32) -> Hct {
        let cam = Cam16::from_int(argb);
        Hct {
            hue: cam.hue,
            chroma: cam.chroma,
            tone: lstar_from_argb(argb),
            argb,
        }
    }

    pub fn from_hct(hue: f64, chroma: f64, tone: f64) -> Hct {
        Hct::from_int(solve_to_int(hue, chroma, tone))
    }

    pub fn to_int(&self) -> u32 {
        self.argb
    }

    pub fn hue(&self) -> f64 {
        self.hue
    }

    pub fn chroma(&self) -> f64 {
        self.chroma
    }

    pub fn tone(&self) -> f64 {
        self.tone
    }

    pub fn set_hue(&mut self, hue: f64) {
        *self = Hct::from_int(solve_to_int(hue, self.chroma, self.tone));
    }

    pub fn set_chroma(&mut self, chroma: f64) {
        *self = Hct::from_int(solve_to_int(self.hue, chroma, self.tone));
    }

    pub fn set_tone(&mut self, tone: f64) {
        *self = Hct::from_int(solve_to_int(self.hue, self.chroma, tone));
    }

    pub fn is_blue(hue: f64) -> bool {
        (250.0..270.0).contains(&hue)
    }

    pub fn is_yellow(hue: f64) -> bool {
        (105.0..125.0).contains(&hue)
    }

    pub fn is_cyan(hue: f64) -> bool {
        (170.0..207.0).contains(&hue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors taken from materialyoucolor itself: the same colours in, the
    /// same numbers out. Anything else here is a palette that looks subtly
    /// wrong on a desktop, so the bar is exactness, not closeness.
    #[test]
    fn agrees_with_the_library_it_replaces() {
        let raw = include_str!("../../tests/hct-vectors.json");
        let Some(vectors) = redcommon::json::parse(raw) else {
            panic!("reference vectors are not readable")
        };

        let mut checked = 0;
        if let Some(redcommon::json::Json::Arr(cases)) = vectors.get("from_int") {
            for case in cases {
                let redcommon::json::Json::Arr(fields) = case else { continue };
                let (argb, hue, chroma, tone) = (
                    number(&fields[0]) as u32,
                    number(&fields[1]),
                    number(&fields[2]),
                    number(&fields[3]),
                );
                let ours = Hct::from_int(argb);
                assert!(
                    (ours.hue() - hue).abs() < 1e-9
                        && (ours.chroma() - chroma).abs() < 1e-9
                        && (ours.tone() - tone).abs() < 1e-9,
                    "{argb:#010x}: got ({}, {}, {}), expected ({hue}, {chroma}, {tone})",
                    ours.hue(),
                    ours.chroma(),
                    ours.tone()
                );
                checked += 1;
            }
        }

        if let Some(redcommon::json::Json::Arr(cases)) = vectors.get("from_hct") {
            for case in cases {
                let redcommon::json::Json::Arr(fields) = case else { continue };
                let (hue, chroma, tone, expected) = (
                    number(&fields[0]),
                    number(&fields[1]),
                    number(&fields[2]),
                    number(&fields[3]) as u32,
                );
                let ours = Hct::from_hct(hue, chroma, tone).to_int();
                assert_eq!(
                    ours, expected,
                    "hct({hue}, {chroma}, {tone}): got {ours:#010x}, expected {expected:#010x}"
                );
                checked += 1;
            }
        }

        assert!(checked > 2000, "only {checked} vectors were checked");
    }

    fn number(value: &redcommon::json::Json) -> f64 {
        match value {
            redcommon::json::Json::Num(n) => *n,
            _ => f64::NAN,
        }
    }
}

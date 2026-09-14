//! A tonal palette: one hue and chroma, every tone from 0 to 100.
//!
//! Ported from materialyoucolor's `palettes/tonal_palette.py`.

use std::collections::HashMap;

use super::hct::Hct;
use super::math::round_half_to_even;

/// The tone at which a hue and chroma are most themselves.
///
/// Walks tones towards 50 looking for the lightest one that still holds the
/// chroma asked for, because a palette's identity should not be a colour the
/// gamut had to flatten.
struct KeyColour {
    hue: f64,
    requested_chroma: f64,
    cache: HashMap<i64, f64>,
}

impl KeyColour {
    fn new(hue: f64, requested_chroma: f64) -> KeyColour {
        KeyColour {
            hue,
            requested_chroma,
            cache: HashMap::new(),
        }
    }

    fn create(&mut self) -> Hct {
        const PIVOT: i64 = 50;
        const STEP: i64 = 1;
        const EPSILON: f64 = 0.01;

        let mut lower = 0i64;
        let mut upper = 100i64;

        while lower < upper {
            let mid = (lower + upper) / 2;
            let ascending = self.max_chroma(mid) < self.max_chroma(mid + STEP);
            let sufficient = self.max_chroma(mid) >= self.requested_chroma - EPSILON;

            if sufficient {
                if (lower - PIVOT).abs() < (upper - PIVOT).abs() {
                    upper = mid;
                } else {
                    if lower == mid {
                        return Hct::from_hct(self.hue, self.requested_chroma, lower as f64);
                    }
                    lower = mid;
                }
            } else if ascending {
                lower = mid + STEP;
            } else {
                upper = mid;
            }
        }

        Hct::from_hct(self.hue, self.requested_chroma, lower as f64)
    }

    /// The most chroma this hue can hold at that tone.
    fn max_chroma(&mut self, tone: i64) -> f64 {
        if let Some(chroma) = self.cache.get(&tone) {
            return *chroma;
        }
        let chroma = Hct::from_hct(self.hue, 200.0, tone as f64).chroma();
        self.cache.insert(tone, chroma);
        chroma
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TonalPalette {
    pub hue: f64,
    pub chroma: f64,
    pub key_colour: Hct,
}

impl TonalPalette {
    pub fn from_int(argb: u32) -> TonalPalette {
        TonalPalette::from_hct(Hct::from_int(argb))
    }

    pub fn from_hct(hct: Hct) -> TonalPalette {
        TonalPalette {
            hue: hct.hue(),
            chroma: hct.chroma(),
            key_colour: hct,
        }
    }

    pub fn from_hue_and_chroma(hue: f64, chroma: f64) -> TonalPalette {
        TonalPalette {
            hue,
            chroma,
            key_colour: KeyColour::new(hue, chroma).create(),
        }
    }

    pub fn tone(&self, tone: f64) -> u32 {
        // Yellow at tone 99 is the one place the gamut misbehaves enough to
        // be worth averaging its neighbours instead.
        if tone == 99.0 && Hct::is_yellow(self.hue) {
            return average(self.tone(98.0), self.tone(100.0));
        }
        Hct::from_hct(self.hue, self.chroma, tone).to_int()
    }

    pub fn get_hct(&self, tone: f64) -> Hct {
        Hct::from_int(self.tone(tone))
    }
}

fn average(a: u32, b: u32) -> u32 {
    let channel = |shift: u32| {
        let first = ((a >> shift) & 0xff) as f64;
        let second = ((b >> shift) & 0xff) as f64;
        (round_half_to_even((first + second) / 2.0) as u32) & 0xff
    };
    (0xff << 24) | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use redcommon::json::Json;

    fn num(value: &Json) -> f64 {
        match value {
            Json::Num(n) => *n,
            _ => f64::NAN,
        }
    }

    #[test]
    fn palettes_match_the_library() {
        let vectors = redcommon::json::parse(include_str!("../../tests/palette-vectors.json"))
            .expect("reference vectors are readable");

        let Some(Json::Arr(keys)) = vectors.get("from_hue_chroma") else {
            panic!("no key colour vectors")
        };
        for case in keys {
            let Json::Arr(fields) = case else { continue };
            let (hue, chroma, expected) = (num(&fields[0]), num(&fields[1]), num(&fields[2]) as u32);
            let ours = TonalPalette::from_hue_and_chroma(hue, chroma);
            assert_eq!(
                ours.key_colour.to_int(),
                expected,
                "key colour for hue {hue}, chroma {chroma}"
            );
        }

        let Some(Json::Arr(rows)) = vectors.get("tone") else { panic!("no tone vectors") };
        for row in rows {
            let Json::Arr(fields) = row else { continue };
            let palette = TonalPalette::from_hue_and_chroma(num(&fields[0]), num(&fields[1]));
            let Json::Arr(tones) = &fields[2] else { continue };
            for pair in tones {
                let Json::Arr(pair) = pair else { continue };
                let (tone, expected) = (num(&pair[0]), num(&pair[1]) as u32);
                assert_eq!(
                    palette.tone(tone),
                    expected,
                    "tone {tone} of hue {}, chroma {}",
                    palette.hue,
                    palette.chroma
                );
            }
        }
    }
}

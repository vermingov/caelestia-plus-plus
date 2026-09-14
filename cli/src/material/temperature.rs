//! Warm and cold, as a number.
//!
//! Two of the nine variants pick their third palette by temperature rather
//! than by a fixed hue rotation: `content` takes an analogous colour a sixth
//! of the way round the temperature wheel, `fidelity` takes the complement —
//! the hue whose relative warmth is the mirror of the source's.
//!
//! Warmth here is Ou, Luo, Woodcock and Wright's model: a colour's distance
//! from the cold axis in CIELAB, weighted by how saturated it is.

use std::collections::HashMap;
use std::f64::consts::PI;

use super::colour::lab_from_argb;
use super::hct::Hct;
use super::math::{round_half_to_even, sanitize_degrees_double, sanitize_degrees_int};

pub struct TemperatureCache {
    input: Hct,
    /// The input's chroma and tone at every whole hue, 0 through 360.
    by_hue: Vec<Hct>,
    /// Keyed by ARGB because that is all the temperature depends on; two hues
    /// that quantise to the same colour share an entry, as they do in Python.
    temps: HashMap<u32, f64>,
    by_temp: Vec<Hct>,
}

impl TemperatureCache {
    pub fn new(input: Hct) -> TemperatureCache {
        let by_hue: Vec<Hct> = (0..=360)
            .map(|hue| Hct::from_hct(hue as f64, input.chroma(), input.tone()))
            .collect();

        let mut temps = HashMap::new();
        for hct in by_hue.iter().chain(std::iter::once(&input)) {
            temps.insert(hct.to_int(), raw_temperature(hct));
        }

        let mut by_temp: Vec<Hct> = by_hue.iter().copied().chain(std::iter::once(input)).collect();
        // Stable, like Python's sort: equal temperatures keep hue order.
        by_temp.sort_by(|a, b| {
            let (ta, tb) = (temps[&a.to_int()], temps[&b.to_int()]);
            ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
        });

        TemperatureCache { input, by_hue, temps, by_temp }
    }

    fn temp_of(&self, hct: &Hct) -> f64 {
        self.temps.get(&hct.to_int()).copied().unwrap_or(0.0)
    }

    pub fn warmest(&self) -> Hct {
        self.by_temp[self.by_temp.len() - 1]
    }

    pub fn coldest(&self) -> Hct {
        self.by_temp[0]
    }

    /// Where a colour sits between the coldest and warmest hue available at
    /// this chroma and tone, as 0..1.
    fn relative_temperature(&self, hct: &Hct) -> f64 {
        let range = self.temp_of(&self.warmest()) - self.temp_of(&self.coldest());
        let from_coldest = self.temp_of(hct) - self.temp_of(&self.coldest());
        if range == 0.0 {
            return 0.5;
        }
        from_coldest / range
    }

    /// Colours spaced evenly around the temperature wheel, not the hue wheel —
    /// so the steps look even rather than measuring even.
    pub fn analogous(&self, count: usize, divisions: usize) -> Vec<Hct> {
        let start_hue = round_half_to_even(self.input.hue()) as i64;
        let start_hct = self.by_hue[start_hue as usize];
        let mut last_temp = self.relative_temperature(&start_hct);
        let mut all_colours = vec![start_hct];

        let mut absolute_total_temp_delta = 0.0;
        for i in 0..360 {
            let hue = sanitize_degrees_int(start_hue + i);
            let temp = self.relative_temperature(&self.by_hue[hue as usize]);
            absolute_total_temp_delta += (temp - last_temp).abs();
            last_temp = temp;
        }

        let mut hue_addend: i64 = 1;
        let temp_step = absolute_total_temp_delta / divisions as f64;
        let mut total_temp_delta = 0.0;
        last_temp = self.relative_temperature(&start_hct);

        while all_colours.len() < divisions {
            let hue = sanitize_degrees_int(start_hue + hue_addend);
            let hct = self.by_hue[hue as usize];
            let temp = self.relative_temperature(&hct);
            total_temp_delta += (temp - last_temp).abs();

            let mut desired = all_colours.len() as f64 * temp_step;
            let mut satisfied = total_temp_delta >= desired;
            let mut index_addend = 1;

            while satisfied && all_colours.len() < divisions {
                all_colours.push(hct);
                desired = (all_colours.len() + index_addend) as f64 * temp_step;
                satisfied = total_temp_delta >= desired;
                index_addend += 1;
            }

            last_temp = temp;
            hue_addend += 1;

            if hue_addend > 360 {
                while all_colours.len() < divisions {
                    all_colours.push(hct);
                }
                break;
            }
        }

        let mut answers = vec![self.input];
        let wrap = |index: i64, len: usize| -> usize {
            let mut index = index;
            while index < 0 {
                index += len as i64;
            }
            if index >= len as i64 {
                index %= len as i64;
            }
            index as usize
        };

        let increase = (count - 1) / 2;
        for i in 1..=increase {
            answers.insert(0, all_colours[wrap(-(i as i64), all_colours.len())]);
        }
        for i in 1..=(count - increase - 1) {
            answers.push(all_colours[wrap(i as i64, all_colours.len())]);
        }
        answers
    }

    /// The hue whose relative warmth mirrors the input's — the colour that
    /// feels opposite, which is not the same as the hue 180° away.
    pub fn complement(&self) -> Hct {
        let coldest_hue = self.coldest().hue();
        let coldest_temp = self.temp_of(&self.coldest());
        let warmest_hue = self.warmest().hue();
        let warmest_temp = self.temp_of(&self.warmest());
        let range = warmest_temp - coldest_temp;

        let coldest_to_warmest = is_between(self.input.hue(), coldest_hue, warmest_hue);
        let start_hue = if coldest_to_warmest { warmest_hue } else { coldest_hue };
        let end_hue = if coldest_to_warmest { coldest_hue } else { warmest_hue };

        let mut smallest_error = 1000.0;
        let mut answer = self.by_hue[round_half_to_even(self.input.hue()) as usize];
        let wanted = 1.0 - self.relative_temperature(&self.input);

        for hue_addend in 0..=360 {
            let hue = sanitize_degrees_double(start_hue + hue_addend as f64);
            if !is_between(hue, start_hue, end_hue) {
                continue;
            }
            let candidate = self.by_hue[round_half_to_even(hue) as usize];
            let relative_temp = (self.temp_of(&candidate) - coldest_temp) / range;
            let error = (wanted - relative_temp).abs();
            if error < smallest_error {
                smallest_error = error;
                answer = candidate;
            }
        }
        answer
    }
}

/// Is `angle` on the arc from `a` to `b`, going the way the arc is written?
fn is_between(angle: f64, a: f64, b: f64) -> bool {
    if a < b {
        a <= angle && angle <= b
    } else {
        a <= angle || angle <= b
    }
}

/// Ou, Luo, Woodcock and Wright's warmth: how far round toward 50° in CIELAB,
/// scaled by saturation. Yellow-orange is the warm pole, its opposite the cold.
fn raw_temperature(colour: &Hct) -> f64 {
    let lab = lab_from_argb(colour.to_int());
    let hue = sanitize_degrees_double(lab[2].atan2(lab[1]) * 180.0 / PI);
    let chroma = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
    -0.5 + 0.02 * chroma.powf(1.07) * (sanitize_degrees_double(hue - 50.0) * PI / 180.0).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference values are the library's own, to four places.
    #[test]
    fn warmth_matches_the_library_at_the_poles() {
        for (argb, expected) in [
            (0xff0000ff, -1.3935),
            (0xffff0000, 2.3514),
            (0xffff8000, 1.7999),
            (0xff00ffff, -1.5974),
        ] {
            let ours = raw_temperature(&Hct::from_int(argb));
            assert!((ours - expected).abs() < 5e-5, "{argb:08x}: {ours} vs {expected}");
        }
    }

    #[test]
    fn an_arc_that_wraps_past_zero_still_contains_its_own_angles() {
        assert!(is_between(10.0, 350.0, 20.0));
        assert!(is_between(355.0, 350.0, 20.0));
        assert!(!is_between(180.0, 350.0, 20.0));
        assert!(is_between(180.0, 20.0, 350.0));
    }

    #[test]
    fn analogous_returns_the_asked_for_count_with_the_input_in_the_middle() {
        let cache = TemperatureCache::new(Hct::from_int(0xff0000ff));
        let colours = cache.analogous(5, 12);
        assert_eq!(colours.len(), 5);
        assert_eq!(colours[2].to_int(), 0xff0000ff, "the input sits at the centre");
    }

    #[test]
    fn the_complement_is_not_simply_the_opposite_hue() {
        let input = Hct::from_int(0xff0000ff);
        let complement = TemperatureCache::new(input).complement();
        let opposite = sanitize_degrees_double(input.hue() + 180.0);
        assert!(
            (complement.hue() - opposite).abs() > 1.0,
            "temperature complement {} should differ from the hue opposite {opposite}",
            complement.hue()
        );
    }
}

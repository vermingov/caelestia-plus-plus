//! A colour that knows what it sits on.
//!
//! Material's colours are not fixed values. Each one is a palette, a wanted
//! tone, and a rule about how far it has to stand off whatever is behind it —
//! resolved against a scheme at the moment it is asked for. `on_surface` is
//! "the neutral palette, far enough from the surface to read", and what that
//! comes out as depends on the surface, the contrast setting and the mode.
//!
//! The fields are closures because that is what they are in the library this
//! is ported from: every one of them takes the scheme. Keeping the same shape
//! keeps the port checkable line by line against the original.

use std::rc::Rc;

use super::contrast;
use super::hct::Hct;
use super::math::{clamp_double, lerp, round_half_to_even};
use super::palette::TonalPalette;
use super::scheme::DynamicScheme;
use super::variant::SpecVersion;

/// The contrast a colour wants against its background, at each of the four
/// settings the user can pick. Anything in between is interpolated.
#[derive(Debug, Clone, Copy)]
pub struct ContrastCurve {
    pub low: f64,
    pub normal: f64,
    pub medium: f64,
    pub high: f64,
}

impl ContrastCurve {
    pub fn new(low: f64, normal: f64, medium: f64, high: f64) -> ContrastCurve {
        ContrastCurve { low, normal, medium, high }
    }

    pub fn get(&self, contrast_level: f64) -> f64 {
        if contrast_level <= -1.0 {
            self.low
        } else if contrast_level < 0.0 {
            lerp(self.low, self.normal, (contrast_level + 1.0) / 1.0)
        } else if contrast_level < 0.5 {
            lerp(self.normal, self.medium, contrast_level / 0.5)
        } else if contrast_level < 1.0 {
            lerp(self.medium, self.high, (contrast_level - 0.5) / 0.5)
        } else {
            self.high
        }
    }
}

/// Which way the second role of a pair moves relative to the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TonePolarity {
    Darker,
    Lighter,
    Nearer,
    Farther,
    RelativeDarker,
    RelativeLighter,
}

/// Whether the gap between a pair is held exactly, or only as a floor/ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaConstraint {
    Exact,
    Nearer,
    Farther,
}

/// Two roles that have to stay a set distance apart in tone — a container and
/// the text on it, say — so neither swallows the other as contrast moves.
#[derive(Clone)]
pub struct ToneDeltaPair {
    pub role_a: DynamicColor,
    pub role_b: DynamicColor,
    pub delta: f64,
    pub polarity: TonePolarity,
    pub stay_together: bool,
    pub constraint: DeltaConstraint,
}

impl ToneDeltaPair {
    pub fn new(
        role_a: DynamicColor,
        role_b: DynamicColor,
        delta: f64,
        polarity: TonePolarity,
        stay_together: bool,
        constraint: DeltaConstraint,
    ) -> ToneDeltaPair {
        ToneDeltaPair { role_a, role_b, delta, polarity, stay_together, constraint }
    }
}

type PaletteFn = Rc<dyn Fn(&DynamicScheme) -> TonalPalette>;
type NumberFn = Rc<dyn Fn(&DynamicScheme) -> f64>;
type ColourFn = Rc<dyn Fn(&DynamicScheme) -> Option<DynamicColor>>;
type CurveFn = Rc<dyn Fn(&DynamicScheme) -> Option<ContrastCurve>>;
type PairFn = Rc<dyn Fn(&DynamicScheme) -> Option<ToneDeltaPair>>;

#[derive(Clone)]
pub struct DynamicColor {
    pub name: &'static str,
    pub is_background: bool,
    pub palette: PaletteFn,
    pub tone: NumberFn,
    pub chroma_multiplier: Option<NumberFn>,
    pub background: Option<ColourFn>,
    pub second_background: Option<ColourFn>,
    pub contrast_curve: Option<CurveFn>,
    pub tone_delta_pair: Option<PairFn>,
}

impl DynamicColor {
    pub fn argb(&self, scheme: &DynamicScheme) -> u32 {
        self.hct(scheme).to_int()
    }

    pub fn hct(&self, scheme: &DynamicScheme) -> Hct {
        match scheme.spec_version {
            SpecVersion::V2021 => hct_2021(scheme, self),
            SpecVersion::V2025 => hct_2025(scheme, self),
        }
    }

    pub fn get_tone(&self, scheme: &DynamicScheme) -> f64 {
        match scheme.spec_version {
            SpecVersion::V2021 => tone_2021(scheme, self),
            SpecVersion::V2025 => tone_2025(scheme, self),
        }
    }

    /// The tone that reads against `bg_tone` at `ratio`, picking the side that
    /// actually gets there — and when neither does, the side that gets closest.
    pub fn foreground_tone(bg_tone: f64, ratio: f64) -> f64 {
        let lighter_tone = contrast::lighter_unsafe(bg_tone, ratio);
        let darker_tone = contrast::darker_unsafe(bg_tone, ratio);
        let lighter_ratio = contrast::ratio_of_tones(lighter_tone, bg_tone);
        let darker_ratio = contrast::ratio_of_tones(darker_tone, bg_tone);

        if DynamicColor::tone_prefers_light_foreground(bg_tone) {
            let negligible_difference =
                (lighter_ratio - darker_ratio).abs() < 0.1 && lighter_ratio < ratio && darker_ratio < ratio;
            if lighter_ratio >= ratio || lighter_ratio >= darker_ratio || negligible_difference {
                lighter_tone
            } else {
                darker_tone
            }
        } else if darker_ratio >= ratio || darker_ratio >= lighter_ratio {
            darker_tone
        } else {
            lighter_tone
        }
    }

    pub fn tone_prefers_light_foreground(tone: f64) -> bool {
        round_half_to_even(tone) < 60.0
    }

    pub fn tone_allows_light_foreground(tone: f64) -> bool {
        round_half_to_even(tone) <= 49.0
    }

    pub fn enable_light_foreground(tone: f64) -> f64 {
        if DynamicColor::tone_prefers_light_foreground(tone) && !DynamicColor::tone_allows_light_foreground(tone) {
            49.0
        } else {
            tone
        }
    }
}

/// Builds a colour a field at a time, so the specs read like the tables they
/// are rather than like eight-argument constructor calls.
pub struct Builder {
    colour: DynamicColor,
    tone_set: bool,
}

pub fn colour(name: &'static str, palette: impl Fn(&DynamicScheme) -> TonalPalette + 'static) -> Builder {
    Builder {
        colour: DynamicColor {
            name,
            is_background: false,
            palette: Rc::new(palette),
            tone: Rc::new(|_| 50.0),
            chroma_multiplier: None,
            background: None,
            second_background: None,
            contrast_curve: None,
            tone_delta_pair: None,
        },
        tone_set: false,
    }
}

impl Builder {
    pub fn tone(mut self, tone: impl Fn(&DynamicScheme) -> f64 + 'static) -> Builder {
        self.colour.tone = Rc::new(tone);
        self.tone_set = true;
        self
    }

    pub fn chroma_multiplier(mut self, multiplier: impl Fn(&DynamicScheme) -> f64 + 'static) -> Builder {
        self.colour.chroma_multiplier = Some(Rc::new(multiplier));
        self
    }

    pub fn is_background(mut self) -> Builder {
        self.colour.is_background = true;
        self
    }

    pub fn background(mut self, background: impl Fn(&DynamicScheme) -> Option<DynamicColor> + 'static) -> Builder {
        self.colour.background = Some(Rc::new(background));
        self
    }

    pub fn second_background(mut self, second: impl Fn(&DynamicScheme) -> Option<DynamicColor> + 'static) -> Builder {
        self.colour.second_background = Some(Rc::new(second));
        self
    }

    pub fn contrast_curve(mut self, curve: impl Fn(&DynamicScheme) -> Option<ContrastCurve> + 'static) -> Builder {
        self.colour.contrast_curve = Some(Rc::new(curve));
        self
    }

    pub fn tone_delta_pair(mut self, pair: impl Fn(&DynamicScheme) -> Option<ToneDeltaPair> + 'static) -> Builder {
        self.colour.tone_delta_pair = Some(Rc::new(pair));
        self
    }

    pub fn build(mut self) -> DynamicColor {
        // No tone of its own means "sit where the background sits" — the
        // contrast rule then moves it off from there.
        if !self.tone_set {
            self.colour.tone = match self.colour.background.clone() {
                Some(background) => Rc::new(move |s| background(s).map(|bg| bg.get_tone(s)).unwrap_or(50.0)),
                None => Rc::new(|_| 50.0),
            };
        }
        self.colour
    }
}

fn call<T>(f: &Option<Rc<dyn Fn(&DynamicScheme) -> Option<T>>>, scheme: &DynamicScheme) -> Option<T> {
    f.as_ref().and_then(|f| f(scheme))
}

fn hct_2021(scheme: &DynamicScheme, colour: &DynamicColor) -> Hct {
    (colour.palette)(scheme).get_hct(colour.get_tone(scheme))
}

/// 2025 takes the palette's hue and chroma directly rather than going through
/// the palette's tone table, which is what lets a colour scale its own chroma.
fn hct_2025(scheme: &DynamicScheme, colour: &DynamicColor) -> Hct {
    let palette = (colour.palette)(scheme);
    let tone = colour.get_tone(scheme);
    let multiplier = colour.chroma_multiplier.as_ref().map_or(1.0, |f| f(scheme));
    Hct::from_hct(palette.hue, palette.chroma * multiplier, tone)
}

fn tone_2021(scheme: &DynamicScheme, colour: &DynamicColor) -> f64 {
    let decreasing_contrast = scheme.contrast_level < 0.0;

    if let Some(pair) = call(&colour.tone_delta_pair, scheme) {
        let ToneDeltaPair { role_a, role_b, delta, polarity, stay_together, .. } = pair;

        let a_is_nearer = polarity == TonePolarity::Nearer
            || (polarity == TonePolarity::Lighter && !scheme.is_dark)
            || (polarity == TonePolarity::Darker && scheme.is_dark);
        let (nearer, farther) = if a_is_nearer { (&role_a, &role_b) } else { (&role_b, &role_a) };
        let am_nearer = colour.name == nearer.name;
        let expansion_dir = if scheme.is_dark { 1.0 } else { -1.0 };
        let mut n_tone = (nearer.tone)(scheme);
        let mut f_tone = (farther.tone)(scheme);

        if colour.background.is_some() && nearer.contrast_curve.is_some() && farther.contrast_curve.is_some() {
            let bg = call(&colour.background, scheme);
            let n_curve = call(&nearer.contrast_curve, scheme);
            let f_curve = call(&farther.contrast_curve, scheme);
            if let (Some(bg), Some(n_curve), Some(f_curve)) = (bg, n_curve, f_curve) {
                let bg_tone = bg.get_tone(scheme);
                let n_contrast = n_curve.get(scheme.contrast_level);
                let f_contrast = f_curve.get(scheme.contrast_level);

                if contrast::ratio_of_tones(bg_tone, n_tone) < n_contrast {
                    n_tone = DynamicColor::foreground_tone(bg_tone, n_contrast);
                }
                if contrast::ratio_of_tones(bg_tone, f_tone) < f_contrast {
                    f_tone = DynamicColor::foreground_tone(bg_tone, f_contrast);
                }
                if decreasing_contrast {
                    n_tone = DynamicColor::foreground_tone(bg_tone, n_contrast);
                    f_tone = DynamicColor::foreground_tone(bg_tone, f_contrast);
                }
            }
        }

        if (f_tone - n_tone) * expansion_dir < delta {
            f_tone = clamp_double(0.0, 100.0, n_tone + delta * expansion_dir);
            if (f_tone - n_tone) * expansion_dir < delta {
                n_tone = clamp_double(0.0, 100.0, f_tone - delta * expansion_dir);
            }
        }

        // 50..60 is the band where nothing reads well against it either way,
        // so the pair is pushed out of it rather than left straddling it.
        if (50.0..60.0).contains(&n_tone) {
            if expansion_dir > 0.0 {
                n_tone = 60.0;
                f_tone = f_tone.max(n_tone + delta * expansion_dir);
            } else {
                n_tone = 49.0;
                f_tone = f_tone.min(n_tone + delta * expansion_dir);
            }
        } else if (50.0..60.0).contains(&f_tone) {
            if stay_together {
                if expansion_dir > 0.0 {
                    n_tone = 60.0;
                    f_tone = f_tone.max(n_tone + delta * expansion_dir);
                } else {
                    n_tone = 49.0;
                    f_tone = f_tone.min(n_tone + delta * expansion_dir);
                }
            } else if expansion_dir > 0.0 {
                f_tone = 60.0;
            } else {
                f_tone = 49.0;
            }
        }

        return if am_nearer { n_tone } else { f_tone };
    }

    let mut answer = (colour.tone)(scheme);
    let (Some(background), Some(curve)) = (call(&colour.background, scheme), call(&colour.contrast_curve, scheme))
    else {
        return answer;
    };

    let bg_tone = background.get_tone(scheme);
    let desired_ratio = curve.get(scheme.contrast_level);

    if contrast::ratio_of_tones(bg_tone, answer) < desired_ratio {
        answer = DynamicColor::foreground_tone(bg_tone, desired_ratio);
    }
    if decreasing_contrast {
        answer = DynamicColor::foreground_tone(bg_tone, desired_ratio);
    }

    if colour.is_background && (50.0..60.0).contains(&answer) {
        answer = if contrast::ratio_of_tones(49.0, bg_tone) >= desired_ratio { 49.0 } else { 60.0 };
    }

    let Some(second) = call(&colour.second_background, scheme) else {
        return answer;
    };

    // Two backgrounds: the tone has to read on both, so it is squeezed between
    // them or pushed past whichever end still works.
    let bg_tone1 = background.get_tone(scheme);
    let bg_tone2 = second.get_tone(scheme);
    let upper = bg_tone1.max(bg_tone2);
    let lower = bg_tone1.min(bg_tone2);

    if contrast::ratio_of_tones(upper, answer) >= desired_ratio
        && contrast::ratio_of_tones(lower, answer) >= desired_ratio
    {
        return answer;
    }

    let light_option = contrast::lighter(upper, desired_ratio);
    let dark_option = contrast::darker(lower, desired_ratio);
    let availables: Vec<f64> = [light_option, dark_option].into_iter().filter(|o| *o != -1.0).collect();

    if DynamicColor::tone_prefers_light_foreground(bg_tone1)
        || DynamicColor::tone_prefers_light_foreground(bg_tone2)
    {
        return if light_option < 0.0 { 100.0 } else { light_option };
    }
    if availables.len() == 1 {
        return availables[0];
    }
    if dark_option < 0.0 { 0.0 } else { dark_option }
}

/// The `*FixedDim` roles are backgrounds that are deliberately allowed to sit
/// in the 50..60 band, so the band-avoidance below skips them by name.
fn is_fixed_dim_name(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    lowered.ends_with("_fixed_dim") || lowered.ends_with("fixeddim")
}

fn tone_2025(scheme: &DynamicScheme, colour: &DynamicColor) -> f64 {
    if let Some(pair) = call(&colour.tone_delta_pair, scheme) {
        let ToneDeltaPair { role_a, role_b, delta, polarity, constraint, .. } = pair;

        let absolute_delta = if polarity == TonePolarity::Darker
            || (polarity == TonePolarity::RelativeLighter && scheme.is_dark)
            || (polarity == TonePolarity::RelativeDarker && !scheme.is_dark)
        {
            -delta
        } else {
            delta
        };

        let am_role_a = colour.name == role_a.name;
        let (self_role, ref_role) = if am_role_a { (&role_a, &role_b) } else { (&role_b, &role_a) };
        let mut self_tone = (self_role.tone)(scheme);
        let ref_tone = ref_role.get_tone(scheme);
        let relative_delta = absolute_delta * if am_role_a { 1.0 } else { -1.0 };

        self_tone = match constraint {
            DeltaConstraint::Exact => clamp_double(0.0, 100.0, ref_tone + relative_delta),
            DeltaConstraint::Nearer => {
                if relative_delta > 0.0 {
                    clamp_double(0.0, 100.0, clamp_double(ref_tone, ref_tone + relative_delta, self_tone))
                } else {
                    clamp_double(0.0, 100.0, clamp_double(ref_tone + relative_delta, ref_tone, self_tone))
                }
            }
            DeltaConstraint::Farther => {
                if relative_delta > 0.0 {
                    clamp_double(ref_tone + relative_delta, 100.0, self_tone)
                } else {
                    clamp_double(0.0, ref_tone + relative_delta, self_tone)
                }
            }
        };

        if let (Some(background), Some(curve)) = (call(&colour.background, scheme), call(&colour.contrast_curve, scheme))
        {
            let bg_tone = background.get_tone(scheme);
            let self_contrast = curve.get(scheme.contrast_level);
            if contrast::ratio_of_tones(bg_tone, self_tone) < self_contrast || scheme.contrast_level < 0.0 {
                self_tone = DynamicColor::foreground_tone(bg_tone, self_contrast);
            }
        }

        if colour.is_background && !is_fixed_dim_name(colour.name) {
            self_tone = if self_tone >= 57.0 {
                clamp_double(65.0, 100.0, self_tone)
            } else {
                clamp_double(0.0, 49.0, self_tone)
            };
        }
        return self_tone;
    }

    let mut answer = (colour.tone)(scheme);
    let (Some(background), Some(curve)) = (call(&colour.background, scheme), call(&colour.contrast_curve, scheme))
    else {
        return answer;
    };

    let bg_tone = background.get_tone(scheme);
    let desired_ratio = curve.get(scheme.contrast_level);

    if contrast::ratio_of_tones(bg_tone, answer) < desired_ratio || scheme.contrast_level < 0.0 {
        answer = DynamicColor::foreground_tone(bg_tone, desired_ratio);
    }

    if colour.is_background && !is_fixed_dim_name(colour.name) {
        answer = if answer >= 57.0 {
            clamp_double(65.0, 100.0, answer)
        } else {
            clamp_double(0.0, 49.0, answer)
        };
    }

    let Some(second) = call(&colour.second_background, scheme) else {
        return answer;
    };

    let bg_tone1 = background.get_tone(scheme);
    let bg_tone2 = second.get_tone(scheme);
    let upper = bg_tone1.max(bg_tone2);
    let lower = bg_tone1.min(bg_tone2);

    if contrast::ratio_of_tones(upper, answer) >= desired_ratio
        && contrast::ratio_of_tones(lower, answer) >= desired_ratio
    {
        return answer;
    }

    let light_option = contrast::lighter(upper, desired_ratio);
    let dark_option = contrast::darker(lower, desired_ratio);
    let availables: Vec<f64> = [light_option, dark_option].into_iter().filter(|o| *o != -1.0).collect();

    if DynamicColor::tone_prefers_light_foreground(bg_tone1)
        || DynamicColor::tone_prefers_light_foreground(bg_tone2)
    {
        return if light_option < 0.0 { 100.0 } else { light_option };
    }
    if availables.len() == 1 {
        return availables[0];
    }
    if dark_option < 0.0 { 0.0 } else { dark_option }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_interpolates_between_the_four_settings() {
        let curve = ContrastCurve::new(1.0, 3.0, 4.5, 7.0);
        assert_eq!(curve.get(-2.0), 1.0, "below the low end it flattens");
        assert_eq!(curve.get(-1.0), 1.0);
        assert_eq!(curve.get(-0.5), 2.0);
        assert_eq!(curve.get(0.0), 3.0);
        assert_eq!(curve.get(0.25), 3.75);
        assert_eq!(curve.get(0.5), 4.5);
        assert_eq!(curve.get(0.75), 5.75);
        assert_eq!(curve.get(1.0), 7.0);
        assert_eq!(curve.get(2.0), 7.0, "above the high end it flattens");
    }

    #[test]
    fn the_unreadable_band_is_rounded_the_way_python_rounds() {
        // Python's round() is banker's rounding, so 59.5 goes to 60 and is not
        // a light-foreground tone, while 58.5 goes to 58 and is.
        assert!(!DynamicColor::tone_prefers_light_foreground(59.5));
        assert!(DynamicColor::tone_prefers_light_foreground(58.5));
        assert!(DynamicColor::tone_allows_light_foreground(49.4));
        assert!(!DynamicColor::tone_allows_light_foreground(49.5), "49.5 rounds to even 50");
    }

    #[test]
    fn light_foreground_is_pulled_out_of_the_dead_band() {
        assert_eq!(DynamicColor::enable_light_foreground(55.0), 49.0);
        assert_eq!(DynamicColor::enable_light_foreground(30.0), 30.0, "already allowed");
        assert_eq!(DynamicColor::enable_light_foreground(80.0), 80.0, "prefers dark");
    }

    #[test]
    fn fixed_dim_roles_are_recognised_by_name() {
        assert!(is_fixed_dim_name("primaryFixedDim"));
        assert!(is_fixed_dim_name("primary_fixed_dim"));
        assert!(!is_fixed_dim_name("primaryFixed"));
        assert!(!is_fixed_dim_name("surfaceDim"));
    }
}

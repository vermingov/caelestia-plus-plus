//! A source colour, expanded into the six palettes a scheme is built from.
//!
//! Everything downstream reads from here: the spec tables name a palette and a
//! tone, and this is what "the primary palette" means for a given variant,
//! mode and platform.

use super::dislike;
use super::hct::Hct;
use super::math::sanitize_degrees_double;
use super::palette::TonalPalette;
use super::temperature::TemperatureCache;
use super::variant::{Platform, SpecVersion, Variant};

pub struct DynamicScheme {
    pub source_colour_hct: Hct,
    pub variant: Variant,
    pub contrast_level: f64,
    pub is_dark: bool,
    pub platform: Platform,
    pub spec_version: SpecVersion,
    pub primary_palette: TonalPalette,
    pub secondary_palette: TonalPalette,
    pub tertiary_palette: TonalPalette,
    pub neutral_palette: TonalPalette,
    pub neutral_variant_palette: TonalPalette,
    pub error_palette: TonalPalette,
}

impl DynamicScheme {
    pub fn new(source: Hct, variant: Variant, contrast_level: f64, is_dark: bool) -> DynamicScheme {
        DynamicScheme::with_platform(source, variant, contrast_level, is_dark, Platform::Phone, SpecVersion::V2025)
    }

    pub fn with_platform(
        source: Hct,
        variant: Variant,
        contrast_level: f64,
        is_dark: bool,
        platform: Platform,
        requested_spec: SpecVersion,
    ) -> DynamicScheme {
        let spec_version = SpecVersion::for_variant(requested_spec, variant);
        let at = Args { variant, source, is_dark, platform, spec_version };

        DynamicScheme {
            source_colour_hct: source,
            variant,
            contrast_level,
            is_dark,
            platform,
            spec_version,
            primary_palette: primary_palette(&at),
            secondary_palette: secondary_palette(&at),
            tertiary_palette: tertiary_palette(&at),
            neutral_palette: neutral_palette(&at),
            neutral_variant_palette: neutral_variant_palette(&at),
            error_palette: error_palette(&at).unwrap_or_else(|| TonalPalette::from_hue_and_chroma(25.0, 84.0)),
        }
    }

    /// The same scheme in light mode at normal contrast, keeping the palettes
    /// already computed. The `*Fixed` roles are defined as "whatever the
    /// container would be there", which is how they stay the same colour in
    /// both modes.
    pub fn light_normal(&self) -> DynamicScheme {
        DynamicScheme {
            source_colour_hct: self.source_colour_hct,
            variant: self.variant,
            contrast_level: 0.0,
            is_dark: false,
            platform: self.platform,
            spec_version: self.spec_version,
            primary_palette: self.primary_palette,
            secondary_palette: self.secondary_palette,
            tertiary_palette: self.tertiary_palette,
            neutral_palette: self.neutral_palette,
            neutral_variant_palette: self.neutral_variant_palette,
            error_palette: self.error_palette,
        }
    }

    /// The hue named for the band the source falls in, or the source's own hue
    /// when it falls in none.
    fn piecewise_hue(source: Hct, breakpoints: &[f64], hues: &[f64]) -> f64 {
        let size = (breakpoints.len() - 1).min(hues.len());
        let source_hue = source.hue();
        for i in 0..size {
            if source_hue >= breakpoints[i] && source_hue < breakpoints[i + 1] {
                return sanitize_degrees_double(hues[i]);
            }
        }
        source_hue
    }

    fn rotated_hue(source: Hct, breakpoints: &[f64], rotations: &[f64]) -> f64 {
        let mut rotation = DynamicScheme::piecewise_hue(source, breakpoints, rotations);
        if (breakpoints.len() - 1).min(rotations.len()) == 0 {
            rotation = 0.0;
        }
        sanitize_degrees_double(source.hue() + rotation)
    }
}

/// What the palette rules get to look at. Bundled because each of the six
/// rules takes the same five things.
struct Args {
    variant: Variant,
    source: Hct,
    is_dark: bool,
    platform: Platform,
    spec_version: SpecVersion,
}

fn is_2025(at: &Args) -> bool {
    at.spec_version == SpecVersion::V2025
}

fn pick(phone: f64, watch: f64, platform: Platform) -> f64 {
    if platform.is_phone() { phone } else { watch }
}

fn primary_palette(at: &Args) -> TonalPalette {
    let hue = at.source.hue();
    if is_2025(at) {
        let chroma = match at.variant {
            Variant::Neutral => {
                if at.platform.is_phone() {
                    if Hct::is_blue(hue) { 12.0 } else { 8.0 }
                } else if Hct::is_blue(hue) {
                    16.0
                } else {
                    12.0
                }
            }
            Variant::TonalSpot => {
                if at.platform.is_phone() && at.is_dark { 26.0 } else { 32.0 }
            }
            Variant::Expressive => {
                if at.platform.is_phone() {
                    if at.is_dark { 36.0 } else { 48.0 }
                } else {
                    40.0
                }
            }
            Variant::Vibrant => pick(74.0, 56.0, at.platform),
            _ => return primary_palette_2021(at),
        };
        return TonalPalette::from_hue_and_chroma(hue, chroma);
    }
    primary_palette_2021(at)
}

fn primary_palette_2021(at: &Args) -> TonalPalette {
    let hue = at.source.hue();
    match at.variant {
        Variant::Content | Variant::Fidelity => TonalPalette::from_hue_and_chroma(hue, at.source.chroma()),
        Variant::FruitSalad => TonalPalette::from_hue_and_chroma(sanitize_degrees_double(hue - 50.0), 48.0),
        Variant::Monochrome => TonalPalette::from_hue_and_chroma(hue, 0.0),
        Variant::Neutral => TonalPalette::from_hue_and_chroma(hue, 12.0),
        Variant::Rainbow => TonalPalette::from_hue_and_chroma(hue, 48.0),
        Variant::TonalSpot => TonalPalette::from_hue_and_chroma(hue, 36.0),
        Variant::Expressive => TonalPalette::from_hue_and_chroma(sanitize_degrees_double(hue + 240.0), 40.0),
        Variant::Vibrant => TonalPalette::from_hue_and_chroma(hue, 200.0),
    }
}

fn secondary_palette(at: &Args) -> TonalPalette {
    let hue = at.source.hue();
    if is_2025(at) {
        match at.variant {
            Variant::Neutral => {
                let chroma = if at.platform.is_phone() {
                    if Hct::is_blue(hue) { 6.0 } else { 4.0 }
                } else if Hct::is_blue(hue) {
                    10.0
                } else {
                    6.0
                };
                return TonalPalette::from_hue_and_chroma(hue, chroma);
            }
            Variant::TonalSpot => return TonalPalette::from_hue_and_chroma(hue, 16.0),
            Variant::Expressive => {
                let rotated = DynamicScheme::rotated_hue(
                    at.source,
                    &[0.0, 105.0, 140.0, 204.0, 253.0, 278.0, 300.0, 333.0, 360.0],
                    &[-160.0, 155.0, -100.0, 96.0, -96.0, -156.0, -165.0, -160.0],
                );
                let chroma = if at.platform.is_phone() && at.is_dark { 16.0 } else { 24.0 };
                return TonalPalette::from_hue_and_chroma(rotated, chroma);
            }
            Variant::Vibrant => {
                let rotated = DynamicScheme::rotated_hue(
                    at.source,
                    &[0.0, 38.0, 105.0, 140.0, 333.0, 360.0],
                    &[-14.0, 10.0, -14.0, 10.0, -14.0],
                );
                return TonalPalette::from_hue_and_chroma(rotated, pick(56.0, 36.0, at.platform));
            }
            _ => {}
        }
    }
    secondary_palette_2021(at)
}

fn secondary_palette_2021(at: &Args) -> TonalPalette {
    let hue = at.source.hue();
    match at.variant {
        Variant::Content | Variant::Fidelity => {
            let chroma = at.source.chroma();
            TonalPalette::from_hue_and_chroma(hue, (chroma - 32.0).max(chroma * 0.5))
        }
        Variant::FruitSalad => TonalPalette::from_hue_and_chroma(sanitize_degrees_double(hue - 50.0), 36.0),
        Variant::Monochrome => TonalPalette::from_hue_and_chroma(hue, 0.0),
        Variant::Neutral => TonalPalette::from_hue_and_chroma(hue, 8.0),
        Variant::Rainbow | Variant::TonalSpot => TonalPalette::from_hue_and_chroma(hue, 16.0),
        Variant::Expressive => TonalPalette::from_hue_and_chroma(
            DynamicScheme::rotated_hue(
                at.source,
                &[0.0, 21.0, 51.0, 121.0, 151.0, 191.0, 271.0, 321.0, 360.0],
                &[45.0, 95.0, 45.0, 20.0, 45.0, 90.0, 45.0, 45.0, 45.0],
            ),
            24.0,
        ),
        Variant::Vibrant => TonalPalette::from_hue_and_chroma(
            DynamicScheme::rotated_hue(
                at.source,
                &[0.0, 41.0, 61.0, 101.0, 131.0, 181.0, 251.0, 301.0, 360.0],
                &[18.0, 15.0, 10.0, 12.0, 15.0, 18.0, 15.0, 12.0, 12.0],
            ),
            24.0,
        ),
    }
}

fn tertiary_palette(at: &Args) -> TonalPalette {
    if is_2025(at) {
        match at.variant {
            Variant::Neutral => {
                let rotated = DynamicScheme::rotated_hue(
                    at.source,
                    &[0.0, 38.0, 105.0, 161.0, 204.0, 278.0, 333.0, 360.0],
                    &[-32.0, 26.0, 10.0, -39.0, 24.0, -15.0, -32.0],
                );
                return TonalPalette::from_hue_and_chroma(rotated, pick(20.0, 36.0, at.platform));
            }
            Variant::TonalSpot => {
                let rotated = DynamicScheme::rotated_hue(
                    at.source,
                    &[0.0, 20.0, 71.0, 161.0, 333.0, 360.0],
                    &[-40.0, 48.0, -32.0, 40.0, -32.0],
                );
                return TonalPalette::from_hue_and_chroma(rotated, pick(28.0, 32.0, at.platform));
            }
            Variant::Expressive => {
                let rotated = DynamicScheme::rotated_hue(
                    at.source,
                    &[0.0, 105.0, 140.0, 204.0, 253.0, 278.0, 300.0, 333.0, 360.0],
                    &[-165.0, 160.0, -105.0, 101.0, -101.0, -160.0, -170.0, -165.0],
                );
                return TonalPalette::from_hue_and_chroma(rotated, 48.0);
            }
            Variant::Vibrant => {
                let rotated = DynamicScheme::rotated_hue(
                    at.source,
                    &[0.0, 38.0, 71.0, 105.0, 140.0, 161.0, 253.0, 333.0, 360.0],
                    &[-72.0, 35.0, 24.0, -24.0, 62.0, 50.0, 62.0, -72.0],
                );
                return TonalPalette::from_hue_and_chroma(rotated, 56.0);
            }
            _ => {}
        }
    }
    tertiary_palette_2021(at)
}

fn tertiary_palette_2021(at: &Args) -> TonalPalette {
    let hue = at.source.hue();
    match at.variant {
        // The one place a palette is chosen by warmth rather than by angle.
        Variant::Content => {
            let analogous = TemperatureCache::new(at.source).analogous(3, 6);
            TonalPalette::from_hct(dislike::fix_if_disliked(analogous[2]))
        }
        Variant::Fidelity => {
            TonalPalette::from_hct(dislike::fix_if_disliked(TemperatureCache::new(at.source).complement()))
        }
        Variant::FruitSalad => TonalPalette::from_hue_and_chroma(hue, 36.0),
        Variant::Monochrome => TonalPalette::from_hue_and_chroma(hue, 0.0),
        Variant::Neutral => TonalPalette::from_hue_and_chroma(hue, 16.0),
        Variant::Rainbow | Variant::TonalSpot => {
            TonalPalette::from_hue_and_chroma(sanitize_degrees_double(hue + 60.0), 24.0)
        }
        Variant::Expressive => TonalPalette::from_hue_and_chroma(
            DynamicScheme::rotated_hue(
                at.source,
                &[0.0, 21.0, 51.0, 121.0, 151.0, 191.0, 271.0, 321.0, 360.0],
                &[120.0, 120.0, 20.0, 45.0, 20.0, 15.0, 20.0, 120.0, 120.0],
            ),
            32.0,
        ),
        Variant::Vibrant => TonalPalette::from_hue_and_chroma(
            DynamicScheme::rotated_hue(
                at.source,
                &[0.0, 41.0, 61.0, 101.0, 131.0, 181.0, 251.0, 301.0, 360.0],
                &[35.0, 30.0, 20.0, 25.0, 30.0, 35.0, 30.0, 25.0, 25.0],
            ),
            32.0,
        ),
    }
}

fn expressive_neutral_hue(source: Hct) -> f64 {
    DynamicScheme::rotated_hue(
        source,
        &[0.0, 71.0, 124.0, 253.0, 278.0, 300.0, 360.0],
        &[10.0, 0.0, 10.0, 0.0, 10.0, 0.0],
    )
}

fn expressive_neutral_chroma(source: Hct, is_dark: bool, platform: Platform) -> f64 {
    let hue = expressive_neutral_hue(source);
    if platform.is_phone() {
        if is_dark {
            if Hct::is_yellow(hue) { 6.0 } else { 14.0 }
        } else {
            18.0
        }
    } else {
        12.0
    }
}

fn vibrant_neutral_hue(source: Hct) -> f64 {
    DynamicScheme::rotated_hue(
        source,
        &[0.0, 38.0, 105.0, 140.0, 333.0, 360.0],
        &[-14.0, 10.0, -14.0, 10.0, -14.0],
    )
}

fn vibrant_neutral_chroma(source: Hct, platform: Platform) -> f64 {
    if platform.is_phone() {
        28.0
    } else if Hct::is_blue(vibrant_neutral_hue(source)) {
        28.0
    } else {
        20.0
    }
}

fn neutral_palette(at: &Args) -> TonalPalette {
    let hue = at.source.hue();
    if is_2025(at) {
        match at.variant {
            Variant::Neutral => return TonalPalette::from_hue_and_chroma(hue, pick(1.4, 6.0, at.platform)),
            Variant::TonalSpot => return TonalPalette::from_hue_and_chroma(hue, pick(5.0, 10.0, at.platform)),
            Variant::Expressive => {
                return TonalPalette::from_hue_and_chroma(
                    expressive_neutral_hue(at.source),
                    expressive_neutral_chroma(at.source, at.is_dark, at.platform),
                )
            }
            Variant::Vibrant => {
                return TonalPalette::from_hue_and_chroma(
                    vibrant_neutral_hue(at.source),
                    vibrant_neutral_chroma(at.source, at.platform),
                )
            }
            _ => {}
        }
    }
    match at.variant {
        Variant::Content | Variant::Fidelity => TonalPalette::from_hue_and_chroma(hue, at.source.chroma() / 8.0),
        Variant::FruitSalad => TonalPalette::from_hue_and_chroma(hue, 10.0),
        Variant::Monochrome | Variant::Rainbow => TonalPalette::from_hue_and_chroma(hue, 0.0),
        Variant::Neutral => TonalPalette::from_hue_and_chroma(hue, 2.0),
        Variant::TonalSpot => TonalPalette::from_hue_and_chroma(hue, 6.0),
        Variant::Expressive => TonalPalette::from_hue_and_chroma(sanitize_degrees_double(hue + 15.0), 8.0),
        Variant::Vibrant => TonalPalette::from_hue_and_chroma(hue, 10.0),
    }
}

fn neutral_variant_palette(at: &Args) -> TonalPalette {
    let hue = at.source.hue();
    if is_2025(at) {
        match at.variant {
            Variant::Neutral => {
                return TonalPalette::from_hue_and_chroma(hue, pick(1.4, 6.0, at.platform) * 2.2)
            }
            Variant::TonalSpot => {
                return TonalPalette::from_hue_and_chroma(hue, pick(5.0, 10.0, at.platform) * 1.7)
            }
            Variant::Expressive => {
                let neutral_hue = expressive_neutral_hue(at.source);
                let chroma = expressive_neutral_chroma(at.source, at.is_dark, at.platform);
                let boost = if (105.0..125.0).contains(&neutral_hue) { 1.6 } else { 2.3 };
                return TonalPalette::from_hue_and_chroma(neutral_hue, chroma * boost);
            }
            Variant::Vibrant => {
                let neutral_hue = vibrant_neutral_hue(at.source);
                let chroma = vibrant_neutral_chroma(at.source, at.platform);
                return TonalPalette::from_hue_and_chroma(neutral_hue, chroma * 1.29);
            }
            _ => {}
        }
    }
    match at.variant {
        Variant::Content | Variant::Fidelity => {
            TonalPalette::from_hue_and_chroma(hue, at.source.chroma() / 8.0 + 4.0)
        }
        Variant::FruitSalad => TonalPalette::from_hue_and_chroma(hue, 16.0),
        Variant::Monochrome | Variant::Rainbow => TonalPalette::from_hue_and_chroma(hue, 0.0),
        Variant::Neutral => TonalPalette::from_hue_and_chroma(hue, 2.0),
        Variant::TonalSpot => TonalPalette::from_hue_and_chroma(hue, 8.0),
        Variant::Expressive => TonalPalette::from_hue_and_chroma(sanitize_degrees_double(hue + 15.0), 12.0),
        Variant::Vibrant => TonalPalette::from_hue_and_chroma(hue, 12.0),
    }
}

fn error_palette(at: &Args) -> Option<TonalPalette> {
    if !is_2025(at) {
        return None;
    }
    // Red, but nudged so it does not collide with a red-ish source colour.
    let error_hue = DynamicScheme::piecewise_hue(
        at.source,
        &[0.0, 3.0, 13.0, 23.0, 33.0, 43.0, 153.0, 273.0, 360.0],
        &[12.0, 22.0, 32.0, 12.0, 22.0, 32.0, 22.0, 12.0],
    );
    let chroma = match at.variant {
        Variant::Neutral => pick(50.0, 40.0, at.platform),
        Variant::TonalSpot => pick(60.0, 48.0, at.platform),
        Variant::Expressive => pick(64.0, 48.0, at.platform),
        Variant::Vibrant => pick(80.0, 60.0, at.platform),
        _ => return None,
    };
    Some(TonalPalette::from_hue_and_chroma(error_hue, chroma))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_four_respecified_variants_get_the_2025_rules() {
        let source = Hct::from_int(0xff4285f4);
        for variant in [Variant::TonalSpot, Variant::Vibrant, Variant::Expressive, Variant::Neutral] {
            let scheme = DynamicScheme::new(source, variant, 0.0, true);
            assert_eq!(scheme.spec_version, SpecVersion::V2025, "{variant:?}");
        }
        for variant in [
            Variant::Content,
            Variant::Fidelity,
            Variant::Monochrome,
            Variant::Rainbow,
            Variant::FruitSalad,
        ] {
            let scheme = DynamicScheme::new(source, variant, 0.0, true);
            assert_eq!(scheme.spec_version, SpecVersion::V2021, "{variant:?}");
        }
    }

    #[test]
    fn a_hue_outside_every_band_is_added_to_itself() {
        // Not a typo in the port: get_rotated_hue falls back to the source hue
        // as the *rotation*, so an unmatched hue doubles. The schemes on disk
        // were generated with that behaviour.
        let source = Hct::from_hct(200.0, 50.0, 50.0);
        let rotated = DynamicScheme::rotated_hue(source, &[0.0, 10.0], &[5.0]);
        assert!((rotated - sanitize_degrees_double(source.hue() * 2.0)).abs() < 1e-9);
    }

    #[test]
    fn monochrome_has_no_chroma_anywhere() {
        let scheme = DynamicScheme::new(Hct::from_int(0xff4285f4), Variant::Monochrome, 0.0, false);
        assert_eq!(scheme.primary_palette.chroma, 0.0);
        assert_eq!(scheme.neutral_palette.chroma, 0.0);
    }

    #[test]
    fn every_palette_matches_the_library() {
        use redcommon::json::Json;

        let vectors = redcommon::json::parse(include_str!("../../tests/scheme-vectors.json"))
            .expect("reference vectors are readable");
        let Some(Json::Arr(rows)) = vectors.get("palettes") else { panic!("no palette vectors") };

        let num = |v: Option<&Json>| match v {
            Some(Json::Num(n)) => *n,
            _ => f64::NAN,
        };

        let mut checked = 0;
        for row in rows {
            let source = Hct::from_int(num(row.get("source")) as u32);
            let name = row.str_field("variant").expect("variant name");
            let variant = Variant::parse(name).expect("known variant");
            let is_dark = row.bool_field("dark", false);
            let scheme = DynamicScheme::new(source, variant, 0.0, is_dark);

            let expected_spec = match row.str_field("spec") {
                Some("2025") => SpecVersion::V2025,
                _ => SpecVersion::V2021,
            };
            assert_eq!(scheme.spec_version, expected_spec, "spec for {name}");

            let Some(palettes) = row.get("palettes") else { panic!("no palettes") };
            for (key, ours) in [
                ("primary", &scheme.primary_palette),
                ("secondary", &scheme.secondary_palette),
                ("tertiary", &scheme.tertiary_palette),
                ("neutral", &scheme.neutral_palette),
                ("neutral_variant", &scheme.neutral_variant_palette),
                ("error", &scheme.error_palette),
            ] {
                let Some(Json::Arr(fields)) = palettes.get(key) else { panic!("no {key} palette") };
                let where_ = format!("{key} palette of {name} {} from {:08x}",
                    if is_dark { "dark" } else { "light" }, source.to_int());
                assert_eq!(ours.hue, num(fields.first()), "hue of the {where_}");
                assert_eq!(ours.chroma, num(fields.get(1)), "chroma of the {where_}");
                assert_eq!(
                    ours.key_colour.to_int(),
                    num(fields.get(2)) as u32,
                    "key colour of the {where_}"
                );
                checked += 1;
            }
        }
        assert!(checked >= 2500, "only {checked} palettes checked");
    }

    #[test]
    fn the_2021_variants_fall_back_to_the_stock_error_red() {
        let scheme = DynamicScheme::new(Hct::from_int(0xff4285f4), Variant::Content, 0.0, false);
        assert_eq!(scheme.error_palette.chroma, 84.0);
        assert_eq!(scheme.error_palette.hue, 25.0);
    }
}

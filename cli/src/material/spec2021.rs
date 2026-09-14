//! The 2021 colour roles.
//!
//! Five of the nine variants still run these rules — `content`, `fidelity`,
//! `monochrome`, `rainbow` and `fruitsalad` — and the four that were
//! respecified fall back to them for the handful of roles 2025 left alone.
//!
//! Every function here is one entry in Material's colour table, in the order
//! the library declares them. Cross-references go through `spec`, never
//! directly to a sibling in this file: the library reaches them through the
//! 2025 delegate, so a role that 2025 overrides has to resolve to the override
//! even when a 2021 role is the one asking.

use super::dislike;
use super::dynamic::{colour, ContrastCurve, DeltaConstraint, DynamicColor, ToneDeltaPair, TonePolarity};
use super::hct::Hct;
use super::scheme::DynamicScheme;
use super::spec;
use super::variant::Variant;

fn is_fidelity(s: &DynamicScheme) -> bool {
    s.variant == Variant::Fidelity || s.variant == Variant::Content
}

fn is_monochrome(s: &DynamicScheme) -> bool {
    s.variant == Variant::Monochrome
}

fn curve(low: f64, normal: f64, medium: f64, high: f64) -> Option<ContrastCurve> {
    Some(ContrastCurve::new(low, normal, medium, high))
}

/// Walk the tone up or down until the palette's chroma is actually reachable.
/// Only `fidelity` and `content` use it, to keep a container as saturated as
/// the source colour was.
fn find_desired_chroma_by_tone(hue: f64, chroma: f64, tone: f64, by_decreasing_tone: bool) -> f64 {
    let mut answer = tone;
    let mut closest = Hct::from_hct(hue, chroma, tone);
    if closest.chroma() < chroma {
        let mut chroma_peak = closest.chroma();
        while closest.chroma() < chroma {
            answer += if by_decreasing_tone { -1.0 } else { 1.0 };
            let candidate = Hct::from_hct(hue, chroma, answer);
            if chroma_peak > candidate.chroma() {
                break;
            }
            if (candidate.chroma() - chroma).abs() < 0.4 {
                break;
            }
            if (candidate.chroma() - chroma).abs() < (closest.chroma() - chroma).abs() {
                closest = candidate;
            }
            chroma_peak = chroma_peak.max(candidate.chroma());
        }
    }
    answer
}

/// A container and its accent, held ten tones apart so neither swallows the
/// other as contrast moves.
fn nearer_pair(container: DynamicColor, accent: DynamicColor) -> Option<ToneDeltaPair> {
    Some(ToneDeltaPair::new(
        container,
        accent,
        10.0,
        TonePolarity::Nearer,
        false,
        DeltaConstraint::Exact,
    ))
}

/// A fixed role and its dim twin, which move together.
fn lighter_pair(fixed: DynamicColor, fixed_dim: DynamicColor) -> Option<ToneDeltaPair> {
    Some(ToneDeltaPair::new(
        fixed,
        fixed_dim,
        10.0,
        TonePolarity::Lighter,
        true,
        DeltaConstraint::Exact,
    ))
}

// ---- palette key colours -------------------------------------------------
// Not drawn with; they are the hue and chroma each palette was built around,
// which the shell exposes so themes can harmonise against them.

pub fn primary_palette_key_colour() -> DynamicColor {
    colour("primaryPaletteKeyColor", |s| s.primary_palette)
        .tone(|s| s.primary_palette.key_colour.tone())
        .build()
}

pub fn secondary_palette_key_colour() -> DynamicColor {
    colour("secondaryPaletteKeyColor", |s| s.secondary_palette)
        .tone(|s| s.secondary_palette.key_colour.tone())
        .build()
}

pub fn tertiary_palette_key_colour() -> DynamicColor {
    colour("tertiaryPaletteKeyColor", |s| s.tertiary_palette)
        .tone(|s| s.tertiary_palette.key_colour.tone())
        .build()
}

pub fn neutral_palette_key_colour() -> DynamicColor {
    colour("neutralPaletteKeyColor", |s| s.neutral_palette)
        .tone(|s| s.neutral_palette.key_colour.tone())
        .build()
}

pub fn neutral_variant_palette_key_colour() -> DynamicColor {
    colour("neutralVariantPaletteKeyColor", |s| s.neutral_variant_palette)
        .tone(|s| s.neutral_variant_palette.key_colour.tone())
        .build()
}

pub fn error_palette_key_colour() -> DynamicColor {
    colour("errorPaletteKeyColor", |s| s.error_palette)
        .tone(|s| s.error_palette.key_colour.tone())
        .build()
}

// ---- surfaces ------------------------------------------------------------

pub fn background() -> DynamicColor {
    colour("background", |s| s.neutral_palette)
        .tone(|s| if s.is_dark { 6.0 } else { 98.0 })
        .is_background()
        .build()
}

pub fn on_background() -> DynamicColor {
    colour("onBackground", |s| s.neutral_palette)
        .tone(|s| if s.is_dark { 90.0 } else { 10.0 })
        .background(|s| Some(spec::background(s)))
        .contrast_curve(|_| curve(3.0, 3.0, 4.5, 7.0))
        .build()
}

pub fn surface() -> DynamicColor {
    colour("surface", |s| s.neutral_palette)
        .tone(|s| if s.is_dark { 6.0 } else { 98.0 })
        .is_background()
        .build()
}

pub fn surface_dim() -> DynamicColor {
    colour("surfaceDim", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                6.0
            } else {
                ContrastCurve::new(87.0, 87.0, 80.0, 75.0).get(s.contrast_level)
            }
        })
        .is_background()
        .build()
}

pub fn surface_bright() -> DynamicColor {
    colour("surfaceBright", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                ContrastCurve::new(24.0, 24.0, 29.0, 34.0).get(s.contrast_level)
            } else {
                98.0
            }
        })
        .is_background()
        .build()
}

pub fn surface_container_lowest() -> DynamicColor {
    colour("surfaceContainerLowest", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                ContrastCurve::new(4.0, 4.0, 2.0, 0.0).get(s.contrast_level)
            } else {
                100.0
            }
        })
        .is_background()
        .build()
}

pub fn surface_container_low() -> DynamicColor {
    colour("surfaceContainerLow", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                ContrastCurve::new(10.0, 10.0, 11.0, 12.0).get(s.contrast_level)
            } else {
                ContrastCurve::new(96.0, 96.0, 96.0, 95.0).get(s.contrast_level)
            }
        })
        .is_background()
        .build()
}

pub fn surface_container() -> DynamicColor {
    colour("surfaceContainer", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                ContrastCurve::new(12.0, 12.0, 16.0, 20.0).get(s.contrast_level)
            } else {
                ContrastCurve::new(94.0, 94.0, 92.0, 90.0).get(s.contrast_level)
            }
        })
        .is_background()
        .build()
}

pub fn surface_container_high() -> DynamicColor {
    colour("surfaceContainerHigh", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                ContrastCurve::new(17.0, 17.0, 21.0, 25.0).get(s.contrast_level)
            } else {
                ContrastCurve::new(92.0, 92.0, 88.0, 85.0).get(s.contrast_level)
            }
        })
        .is_background()
        .build()
}

pub fn surface_container_highest() -> DynamicColor {
    colour("surfaceContainerHighest", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                ContrastCurve::new(22.0, 22.0, 26.0, 30.0).get(s.contrast_level)
            } else {
                ContrastCurve::new(90.0, 90.0, 84.0, 80.0).get(s.contrast_level)
            }
        })
        .is_background()
        .build()
}

pub fn on_surface() -> DynamicColor {
    colour("onSurface", |s| s.neutral_palette)
        .tone(|s| if s.is_dark { 90.0 } else { 10.0 })
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(4.5, 7.0, 11.0, 21.0))
        .build()
}

pub fn surface_variant() -> DynamicColor {
    colour("surfaceVariant", |s| s.neutral_variant_palette)
        .tone(|s| if s.is_dark { 30.0 } else { 90.0 })
        .is_background()
        .build()
}

pub fn on_surface_variant() -> DynamicColor {
    colour("onSurfaceVariant", |s| s.neutral_variant_palette)
        .tone(|s| if s.is_dark { 80.0 } else { 30.0 })
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 11.0))
        .build()
}

pub fn inverse_surface() -> DynamicColor {
    colour("inverseSurface", |s| s.neutral_palette)
        .tone(|s| if s.is_dark { 90.0 } else { 20.0 })
        .is_background()
        .build()
}

pub fn inverse_on_surface() -> DynamicColor {
    colour("inverseOnSurface", |s| s.neutral_palette)
        .tone(|s| if s.is_dark { 20.0 } else { 95.0 })
        .background(|s| Some(spec::inverse_surface(s)))
        .contrast_curve(|_| curve(4.5, 7.0, 11.0, 21.0))
        .build()
}

pub fn outline() -> DynamicColor {
    colour("outline", |s| s.neutral_variant_palette)
        .tone(|s| if s.is_dark { 60.0 } else { 50.0 })
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.5, 3.0, 4.5, 7.0))
        .build()
}

pub fn outline_variant() -> DynamicColor {
    colour("outlineVariant", |s| s.neutral_variant_palette)
        .tone(|s| if s.is_dark { 30.0 } else { 80.0 })
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .build()
}

pub fn shadow() -> DynamicColor {
    colour("shadow", |s| s.neutral_palette).tone(|_| 0.0).build()
}

pub fn scrim() -> DynamicColor {
    colour("scrim", |s| s.neutral_palette).tone(|_| 0.0).build()
}

pub fn surface_tint() -> DynamicColor {
    colour("surfaceTint", |s| s.primary_palette)
        .tone(|s| if s.is_dark { 80.0 } else { 40.0 })
        .is_background()
        .build()
}

// ---- primary -------------------------------------------------------------

pub fn primary() -> DynamicColor {
    colour("primary", |s| s.primary_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 100.0 } else { 0.0 }
            } else if s.is_dark {
                80.0
            } else {
                40.0
            }
        })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 7.0))
        .tone_delta_pair(|s| nearer_pair(spec::primary_container(s), spec::primary(s)))
        .build()
}

pub fn on_primary() -> DynamicColor {
    colour("onPrimary", |s| s.primary_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 10.0 } else { 90.0 }
            } else if s.is_dark {
                20.0
            } else {
                100.0
            }
        })
        .background(|s| Some(spec::primary(s)))
        .contrast_curve(|_| curve(4.5, 7.0, 11.0, 21.0))
        .build()
}

pub fn primary_container() -> DynamicColor {
    colour("primaryContainer", |s| s.primary_palette)
        .tone(|s| {
            if is_fidelity(s) {
                s.source_colour_hct.tone()
            } else if is_monochrome(s) {
                if s.is_dark { 85.0 } else { 25.0 }
            } else if s.is_dark {
                30.0
            } else {
                90.0
            }
        })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| nearer_pair(spec::primary_container(s), spec::primary(s)))
        .build()
}

pub fn on_primary_container() -> DynamicColor {
    colour("onPrimaryContainer", |s| s.primary_palette)
        .tone(|s| {
            if is_fidelity(s) {
                DynamicColor::foreground_tone((spec::primary_container(s).tone)(s), 4.5)
            } else if is_monochrome(s) {
                if s.is_dark { 0.0 } else { 100.0 }
            } else if s.is_dark {
                90.0
            } else {
                30.0
            }
        })
        .background(|s| Some(spec::primary_container(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 11.0))
        .build()
}

pub fn inverse_primary() -> DynamicColor {
    colour("inversePrimary", |s| s.primary_palette)
        .tone(|s| if s.is_dark { 40.0 } else { 80.0 })
        .background(|s| Some(spec::inverse_surface(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 7.0))
        .build()
}

// ---- secondary -----------------------------------------------------------

pub fn secondary() -> DynamicColor {
    colour("secondary", |s| s.secondary_palette)
        .tone(|s| if s.is_dark { 80.0 } else { 40.0 })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 7.0))
        .tone_delta_pair(|s| nearer_pair(spec::secondary_container(s), spec::secondary(s)))
        .build()
}

pub fn on_secondary() -> DynamicColor {
    colour("onSecondary", |s| s.secondary_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 10.0 } else { 100.0 }
            } else if s.is_dark {
                20.0
            } else {
                100.0
            }
        })
        .background(|s| Some(spec::secondary(s)))
        .contrast_curve(|_| curve(4.5, 7.0, 11.0, 21.0))
        .build()
}

pub fn secondary_container() -> DynamicColor {
    colour("secondaryContainer", |s| s.secondary_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 30.0 } else { 85.0 }
            } else if is_fidelity(s) {
                find_desired_chroma_by_tone(
                    s.secondary_palette.hue,
                    s.secondary_palette.chroma,
                    if s.is_dark { 30.0 } else { 90.0 },
                    !s.is_dark,
                )
            } else if s.is_dark {
                30.0
            } else {
                90.0
            }
        })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| nearer_pair(spec::secondary_container(s), spec::secondary(s)))
        .build()
}

pub fn on_secondary_container() -> DynamicColor {
    colour("onSecondaryContainer", |s| s.secondary_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 90.0 } else { 10.0 }
            } else if is_fidelity(s) {
                DynamicColor::foreground_tone((spec::secondary_container(s).tone)(s), 4.5)
            } else if s.is_dark {
                90.0
            } else {
                30.0
            }
        })
        .background(|s| Some(spec::secondary_container(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 11.0))
        .build()
}

// ---- tertiary ------------------------------------------------------------

pub fn tertiary() -> DynamicColor {
    colour("tertiary", |s| s.tertiary_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 90.0 } else { 25.0 }
            } else if s.is_dark {
                80.0
            } else {
                40.0
            }
        })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 7.0))
        .tone_delta_pair(|s| nearer_pair(spec::tertiary_container(s), spec::tertiary(s)))
        .build()
}

pub fn on_tertiary() -> DynamicColor {
    colour("onTertiary", |s| s.tertiary_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 10.0 } else { 90.0 }
            } else if s.is_dark {
                20.0
            } else {
                100.0
            }
        })
        .background(|s| Some(spec::tertiary(s)))
        .contrast_curve(|_| curve(4.5, 7.0, 11.0, 21.0))
        .build()
}

pub fn tertiary_container() -> DynamicColor {
    colour("tertiaryContainer", |s| s.tertiary_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 60.0 } else { 49.0 }
            } else if is_fidelity(s) {
                dislike::fix_if_disliked(s.tertiary_palette.get_hct(s.source_colour_hct.tone())).tone()
            } else if s.is_dark {
                30.0
            } else {
                90.0
            }
        })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| nearer_pair(spec::tertiary_container(s), spec::tertiary(s)))
        .build()
}

pub fn on_tertiary_container() -> DynamicColor {
    colour("onTertiaryContainer", |s| s.tertiary_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 0.0 } else { 100.0 }
            } else if is_fidelity(s) {
                DynamicColor::foreground_tone((spec::tertiary_container(s).tone)(s), 4.5)
            } else if s.is_dark {
                90.0
            } else {
                30.0
            }
        })
        .background(|s| Some(spec::tertiary_container(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 11.0))
        .build()
}

// ---- error ---------------------------------------------------------------

pub fn error() -> DynamicColor {
    colour("error", |s| s.error_palette)
        .tone(|s| if s.is_dark { 80.0 } else { 40.0 })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 7.0))
        .tone_delta_pair(|s| nearer_pair(spec::error_container(s), spec::error(s)))
        .build()
}

pub fn on_error() -> DynamicColor {
    colour("onError", |s| s.error_palette)
        .tone(|s| if s.is_dark { 20.0 } else { 100.0 })
        .background(|s| Some(spec::error(s)))
        .contrast_curve(|_| curve(4.5, 7.0, 11.0, 21.0))
        .build()
}

pub fn error_container() -> DynamicColor {
    colour("errorContainer", |s| s.error_palette)
        .tone(|s| if s.is_dark { 30.0 } else { 90.0 })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| nearer_pair(spec::error_container(s), spec::error(s)))
        .build()
}

pub fn on_error_container() -> DynamicColor {
    colour("onErrorContainer", |s| s.error_palette)
        .tone(|s| {
            if is_monochrome(s) {
                if s.is_dark { 90.0 } else { 10.0 }
            } else if s.is_dark {
                90.0
            } else {
                30.0
            }
        })
        .background(|s| Some(spec::error_container(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 11.0))
        .build()
}

// ---- fixed roles ---------------------------------------------------------
// The one family whose tones do not flip between light and dark, so a colour
// can be shared across both without being recomputed.

pub fn primary_fixed() -> DynamicColor {
    colour("primaryFixed", |s| s.primary_palette)
        .tone(|s| if is_monochrome(s) { 40.0 } else { 90.0 })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| lighter_pair(spec::primary_fixed(s), spec::primary_fixed_dim(s)))
        .build()
}

pub fn primary_fixed_dim() -> DynamicColor {
    colour("primaryFixedDim", |s| s.primary_palette)
        .tone(|s| if is_monochrome(s) { 30.0 } else { 80.0 })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| lighter_pair(spec::primary_fixed(s), spec::primary_fixed_dim(s)))
        .build()
}

pub fn on_primary_fixed() -> DynamicColor {
    colour("onPrimaryFixed", |s| s.primary_palette)
        .tone(|s| if is_monochrome(s) { 100.0 } else { 10.0 })
        .background(|s| Some(spec::primary_fixed_dim(s)))
        .second_background(|s| Some(spec::primary_fixed(s)))
        .contrast_curve(|_| curve(4.5, 7.0, 11.0, 21.0))
        .build()
}

pub fn on_primary_fixed_variant() -> DynamicColor {
    colour("onPrimaryFixedVariant", |s| s.primary_palette)
        .tone(|s| if is_monochrome(s) { 90.0 } else { 30.0 })
        .background(|s| Some(spec::primary_fixed_dim(s)))
        .second_background(|s| Some(spec::primary_fixed(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 11.0))
        .build()
}

pub fn secondary_fixed() -> DynamicColor {
    colour("secondaryFixed", |s| s.secondary_palette)
        .tone(|s| if is_monochrome(s) { 80.0 } else { 90.0 })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| lighter_pair(spec::secondary_fixed(s), spec::secondary_fixed_dim(s)))
        .build()
}

pub fn secondary_fixed_dim() -> DynamicColor {
    colour("secondaryFixedDim", |s| s.secondary_palette)
        .tone(|s| if is_monochrome(s) { 70.0 } else { 80.0 })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| lighter_pair(spec::secondary_fixed(s), spec::secondary_fixed_dim(s)))
        .build()
}

pub fn on_secondary_fixed() -> DynamicColor {
    colour("onSecondaryFixed", |s| s.secondary_palette)
        .tone(|_| 10.0)
        .background(|s| Some(spec::secondary_fixed_dim(s)))
        .second_background(|s| Some(spec::secondary_fixed(s)))
        .contrast_curve(|_| curve(4.5, 7.0, 11.0, 21.0))
        .build()
}

pub fn on_secondary_fixed_variant() -> DynamicColor {
    colour("onSecondaryFixedVariant", |s| s.secondary_palette)
        .tone(|s| if is_monochrome(s) { 25.0 } else { 30.0 })
        .background(|s| Some(spec::secondary_fixed_dim(s)))
        .second_background(|s| Some(spec::secondary_fixed(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 11.0))
        .build()
}

pub fn tertiary_fixed() -> DynamicColor {
    colour("tertiaryFixed", |s| s.tertiary_palette)
        .tone(|s| if is_monochrome(s) { 40.0 } else { 90.0 })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| lighter_pair(spec::tertiary_fixed(s), spec::tertiary_fixed_dim(s)))
        .build()
}

pub fn tertiary_fixed_dim() -> DynamicColor {
    colour("tertiaryFixedDim", |s| s.tertiary_palette)
        .tone(|s| if is_monochrome(s) { 30.0 } else { 80.0 })
        .is_background()
        .background(|s| Some(spec::highest_surface(s)))
        .contrast_curve(|_| curve(1.0, 1.0, 3.0, 4.5))
        .tone_delta_pair(|s| lighter_pair(spec::tertiary_fixed(s), spec::tertiary_fixed_dim(s)))
        .build()
}

pub fn on_tertiary_fixed() -> DynamicColor {
    colour("onTertiaryFixed", |s| s.tertiary_palette)
        .tone(|s| if is_monochrome(s) { 100.0 } else { 10.0 })
        .background(|s| Some(spec::tertiary_fixed_dim(s)))
        .second_background(|s| Some(spec::tertiary_fixed(s)))
        .contrast_curve(|_| curve(4.5, 7.0, 11.0, 21.0))
        .build()
}

pub fn on_tertiary_fixed_variant() -> DynamicColor {
    colour("onTertiaryFixedVariant", |s| s.tertiary_palette)
        .tone(|s| if is_monochrome(s) { 90.0 } else { 30.0 })
        .background(|s| Some(spec::tertiary_fixed_dim(s)))
        .second_background(|s| Some(spec::tertiary_fixed(s)))
        .contrast_curve(|_| curve(3.0, 4.5, 7.0, 11.0))
        .build()
}

/// What a foreground actually sits on: the brightest surface in dark mode, the
/// dimmest in light. Both are the same neutral palette at different tones.
pub fn highest_surface(s: &DynamicScheme) -> DynamicColor {
    if s.is_dark {
        spec::surface_bright(s)
    } else {
        spec::surface_dim(s)
    }
}

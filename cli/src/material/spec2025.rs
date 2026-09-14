//! The 2025 colour roles.
//!
//! Four variants run these: `tonalspot`, `vibrant`, `expressive`, `neutral`.
//! The difference from 2021 is that a role no longer names a fixed tone — it
//! names the most saturated tone the palette can reach inside a window, and
//! scales the palette's chroma on the way. That is what makes 2025 surfaces
//! carry a tint instead of being flat grey.
//!
//! Roles this file does not define fall through to `spec2021`; the dispatch
//! lives in `spec`.

use super::dynamic::{colour, ContrastCurve, DeltaConstraint, DynamicColor, ToneDeltaPair, TonePolarity};
use super::hct::Hct;
use super::math::clamp_double;
use super::palette::TonalPalette;
use super::scheme::DynamicScheme;
use super::spec;
use super::variant::Variant;

/// The most saturated tone this palette reaches, searching upward from black.
fn t_max_c(palette: TonalPalette, lower: f64, upper: f64, chroma_multiplier: f64) -> f64 {
    let answer = best_tone_for_chroma(palette.hue, palette.chroma * chroma_multiplier, 100.0, true);
    clamp_double(lower, upper, answer)
}

/// The same search from the other end, which lands on a darker tone of the
/// same hue.
fn t_min_c(palette: TonalPalette, lower: f64, upper: f64) -> f64 {
    let answer = best_tone_for_chroma(palette.hue, palette.chroma, 0.0, false);
    clamp_double(lower, upper, answer)
}

fn best_tone_for_chroma(hue: f64, chroma: f64, tone: f64, by_decreasing_tone: bool) -> f64 {
    let mut tone = tone;
    let mut answer = tone;
    let mut best = Hct::from_hct(hue, chroma, answer);
    while best.chroma() < chroma {
        if !(0.0..=100.0).contains(&tone) {
            break;
        }
        tone += if by_decreasing_tone { -1.0 } else { 1.0 };
        let candidate = Hct::from_hct(hue, chroma, tone);
        if best.chroma() < candidate.chroma() {
            best = candidate;
            answer = tone;
        }
    }
    answer
}

/// 2025 names a contrast ratio and takes the whole curve from a table, rather
/// than spelling out four numbers per role.
fn get_curve(default_contrast: f64) -> ContrastCurve {
    let (medium, high) = match default_contrast {
        c if c == 1.5 => (3.0, 5.5),
        c if c == 3.0 => (4.5, 7.0),
        c if c == 4.5 => (7.0, 11.0),
        c if c == 6.0 => (7.0, 11.0),
        c if c == 7.0 => (11.0, 21.0),
        c if c == 9.0 => (11.0, 21.0),
        c if c == 11.0 => (21.0, 21.0),
        c if c == 21.0 => (21.0, 21.0),
        _ => (7.0, 21.0),
    };
    ContrastCurve::new(default_contrast, default_contrast, medium, high)
}

fn neutral_is_yellow(s: &DynamicScheme) -> bool {
    Hct::is_yellow(s.neutral_palette.hue)
}

/// A surface's tint strength. Every surface role uses the same shape with its
/// own five numbers, so they share this and pass theirs in. Anything that is
/// not one of the four respecified variants is untinted.
fn surface_chroma_multiplier(
    s: &DynamicScheme,
    neutral: f64,
    tonal_spot: f64,
    expressive_yellow: f64,
    expressive: f64,
    vibrant: f64,
) -> f64 {
    match s.variant {
        Variant::Neutral => neutral,
        Variant::TonalSpot => tonal_spot,
        Variant::Expressive => {
            if neutral_is_yellow(s) {
                expressive_yellow
            } else {
                expressive
            }
        }
        Variant::Vibrant => vibrant,
        _ => 1.0,
    }
}

/// The multiplier shared by `onSurface`, `onSurfaceVariant`, `outline` and
/// `outlineVariant` — the only one that also looks at the mode.
fn on_surface_family_chroma_multiplier(s: &DynamicScheme) -> f64 {
    if !s.platform.is_phone() {
        return 1.0;
    }
    match s.variant {
        Variant::Neutral => 2.2,
        Variant::TonalSpot => 1.7,
        Variant::Expressive => {
            if neutral_is_yellow(s) && s.is_dark {
                3.0
            } else if neutral_is_yellow(s) {
                2.3
            } else {
                1.6
            }
        }
        _ => 1.0,
    }
}

/// What a foreground sits on: the extreme surface on a phone, the high
/// container on a watch.
fn on_surface_background(s: &DynamicScheme) -> Option<DynamicColor> {
    Some(if s.platform.is_phone() {
        spec::highest_surface(s)
    } else {
        spec::surface_container_high(s)
    })
}

/// A container is only measured against a background on a phone; on a watch it
/// is positioned by its pair with the dim role instead.
fn container_background(s: &DynamicScheme) -> Option<DynamicColor> {
    if s.platform.is_phone() {
        Some(spec::highest_surface(s))
    } else {
        None
    }
}

/// Containers only take on a contrast requirement once the user asks for more
/// contrast than normal.
fn container_contrast_curve(s: &DynamicScheme) -> Option<ContrastCurve> {
    if s.platform.is_phone() && s.contrast_level > 0.0 {
        Some(get_curve(1.5))
    } else {
        None
    }
}

/// An accent and its container, five tones apart and moving together.
fn accent_pair(container: DynamicColor, accent: DynamicColor) -> Option<ToneDeltaPair> {
    Some(ToneDeltaPair::new(
        container,
        accent,
        5.0,
        TonePolarity::RelativeLighter,
        true,
        DeltaConstraint::Farther,
    ))
}

fn dim_pair(dim: DynamicColor, accent: DynamicColor) -> Option<ToneDeltaPair> {
    Some(ToneDeltaPair::new(
        dim,
        accent,
        5.0,
        TonePolarity::Darker,
        true,
        DeltaConstraint::Farther,
    ))
}

fn watch_container_pair(container: DynamicColor, dim: DynamicColor) -> Option<ToneDeltaPair> {
    Some(ToneDeltaPair::new(
        container,
        dim,
        10.0,
        TonePolarity::Darker,
        true,
        DeltaConstraint::Farther,
    ))
}

fn fixed_dim_pair(fixed_dim: DynamicColor, fixed: DynamicColor) -> Option<ToneDeltaPair> {
    Some(ToneDeltaPair::new(
        fixed_dim,
        fixed,
        5.0,
        TonePolarity::Darker,
        true,
        DeltaConstraint::Exact,
    ))
}

// ---- surfaces ------------------------------------------------------------

pub fn surface() -> DynamicColor {
    colour("surface", |s| s.neutral_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                0.0
            } else if s.is_dark {
                4.0
            } else if neutral_is_yellow(s) {
                99.0
            } else if s.variant == Variant::Vibrant {
                97.0
            } else {
                98.0
            }
        })
        .is_background()
        .build()
}

pub fn surface_dim() -> DynamicColor {
    colour("surfaceDim", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                4.0
            } else if neutral_is_yellow(s) {
                90.0
            } else if s.variant == Variant::Vibrant {
                85.0
            } else {
                87.0
            }
        })
        .is_background()
        .chroma_multiplier(|s| {
            if s.is_dark {
                return 1.0;
            }
            surface_chroma_multiplier(s, 2.5, 1.7, 2.7, 1.75, 1.36)
        })
        .build()
}

pub fn surface_bright() -> DynamicColor {
    colour("surfaceBright", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                18.0
            } else if neutral_is_yellow(s) {
                99.0
            } else if s.variant == Variant::Vibrant {
                97.0
            } else {
                98.0
            }
        })
        .is_background()
        .chroma_multiplier(|s| {
            if !s.is_dark {
                return 1.0;
            }
            surface_chroma_multiplier(s, 2.5, 1.7, 2.7, 1.75, 1.36)
        })
        .build()
}

pub fn surface_container_lowest() -> DynamicColor {
    colour("surfaceContainerLowest", |s| s.neutral_palette)
        .tone(|s| if s.is_dark { 0.0 } else { 100.0 })
        .is_background()
        .build()
}

pub fn surface_container_low() -> DynamicColor {
    colour("surfaceContainerLow", |s| s.neutral_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                15.0
            } else if s.is_dark {
                6.0
            } else if neutral_is_yellow(s) {
                98.0
            } else if s.variant == Variant::Vibrant {
                95.0
            } else {
                96.0
            }
        })
        .is_background()
        .chroma_multiplier(|s| {
            if !s.platform.is_phone() {
                return 1.0;
            }
            surface_chroma_multiplier(s, 1.3, 1.25, 1.3, 1.15, 1.08)
        })
        .build()
}

pub fn surface_container() -> DynamicColor {
    colour("surfaceContainer", |s| s.neutral_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                20.0
            } else if s.is_dark {
                9.0
            } else if neutral_is_yellow(s) {
                96.0
            } else if s.variant == Variant::Vibrant {
                92.0
            } else {
                94.0
            }
        })
        .is_background()
        .chroma_multiplier(|s| {
            if !s.platform.is_phone() {
                return 1.0;
            }
            surface_chroma_multiplier(s, 1.6, 1.4, 1.6, 1.3, 1.15)
        })
        .build()
}

pub fn surface_container_high() -> DynamicColor {
    colour("surfaceContainerHigh", |s| s.neutral_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                25.0
            } else if s.is_dark {
                12.0
            } else if neutral_is_yellow(s) {
                94.0
            } else if s.variant == Variant::Vibrant {
                90.0
            } else {
                92.0
            }
        })
        .is_background()
        .chroma_multiplier(|s| {
            if !s.platform.is_phone() {
                return 1.0;
            }
            surface_chroma_multiplier(s, 1.9, 1.5, 1.95, 1.45, 1.22)
        })
        .build()
}

pub fn surface_container_highest() -> DynamicColor {
    colour("surfaceContainerHighest", |s| s.neutral_palette)
        .tone(|s| {
            if s.is_dark {
                15.0
            } else if neutral_is_yellow(s) {
                92.0
            } else if s.variant == Variant::Vibrant {
                88.0
            } else {
                90.0
            }
        })
        .is_background()
        .chroma_multiplier(|s| {
            surface_chroma_multiplier(s, 2.2, 1.7, 2.3, 1.6, 1.29)
        })
        .build()
}

pub fn on_surface() -> DynamicColor {
    colour("onSurface", |s| s.neutral_palette)
        .tone(|s| {
            if s.variant == Variant::Vibrant {
                t_max_c(s.neutral_palette, 0.0, 100.0, 1.1)
            } else {
                on_surface_background(s).map_or(50.0, |bg| bg.get_tone(s))
            }
        })
        .chroma_multiplier(on_surface_family_chroma_multiplier)
        .background(on_surface_background)
        .contrast_curve(|s| {
            Some(if s.is_dark && s.platform.is_phone() { get_curve(11.0) } else { get_curve(9.0) })
        })
        .build()
}

pub fn on_surface_variant() -> DynamicColor {
    colour("onSurfaceVariant", |s| s.neutral_palette)
        .chroma_multiplier(on_surface_family_chroma_multiplier)
        .background(on_surface_background)
        .contrast_curve(|s| {
            Some(if s.platform.is_phone() && s.is_dark {
                get_curve(6.0)
            } else if s.platform.is_phone() {
                get_curve(4.5)
            } else {
                get_curve(7.0)
            })
        })
        .build()
}

pub fn outline() -> DynamicColor {
    colour("outline", |s| s.neutral_palette)
        .chroma_multiplier(on_surface_family_chroma_multiplier)
        .background(on_surface_background)
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(3.0) } else { get_curve(4.5) }))
        .build()
}

pub fn outline_variant() -> DynamicColor {
    colour("outlineVariant", |s| s.neutral_palette)
        .chroma_multiplier(on_surface_family_chroma_multiplier)
        .background(on_surface_background)
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(1.5) } else { get_curve(3.0) }))
        .build()
}

pub fn inverse_surface() -> DynamicColor {
    colour("inverseSurface", |s| s.neutral_palette)
        .tone(|s| if s.is_dark { 98.0 } else { 4.0 })
        .is_background()
        .build()
}

pub fn inverse_on_surface() -> DynamicColor {
    colour("inverseOnSurface", |s| s.neutral_palette)
        .background(|s| Some(spec::inverse_surface(s)))
        .contrast_curve(|_| Some(get_curve(7.0)))
        .build()
}

// ---- primary -------------------------------------------------------------

pub fn primary() -> DynamicColor {
    colour("primary", |s| s.primary_palette)
        .tone(|s| match s.variant {
            Variant::Neutral => {
                if s.platform.is_phone() && s.is_dark {
                    80.0
                } else if s.platform.is_phone() {
                    40.0
                } else {
                    90.0
                }
            }
            Variant::TonalSpot => {
                if s.platform.is_phone() && s.is_dark {
                    80.0
                } else if s.platform.is_phone() {
                    t_max_c(s.primary_palette, 0.0, 100.0, 1.0)
                } else {
                    t_max_c(s.primary_palette, 0.0, 90.0, 1.0)
                }
            }
            Variant::Expressive => {
                if !s.platform.is_phone() {
                    t_max_c(s.primary_palette, 0.0, 100.0, 1.0)
                } else if Hct::is_yellow(s.primary_palette.hue) {
                    t_max_c(s.primary_palette, 0.0, 25.0, 1.0)
                } else if Hct::is_cyan(s.primary_palette.hue) {
                    t_max_c(s.primary_palette, 0.0, 88.0, 1.0)
                } else {
                    t_max_c(s.primary_palette, 0.0, 98.0, 1.0)
                }
            }
            Variant::Vibrant => {
                if !s.platform.is_phone() {
                    t_max_c(s.primary_palette, 0.0, 100.0, 1.0)
                } else if Hct::is_cyan(s.primary_palette.hue) {
                    t_max_c(s.primary_palette, 0.0, 88.0, 1.0)
                } else {
                    t_max_c(s.primary_palette, 0.0, 98.0, 1.0)
                }
            }
            _ => 0.0,
        })
        .is_background()
        .background(on_surface_background)
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(4.5) } else { get_curve(7.0) }))
        .tone_delta_pair(|s| {
            if s.platform.is_phone() {
                accent_pair(spec::primary_container(s), spec::primary(s))
            } else {
                None
            }
        })
        .build()
}

pub fn primary_dim() -> DynamicColor {
    colour("primaryDim", |s| s.primary_palette)
        .tone(|s| match s.variant {
            Variant::Neutral => 85.0,
            Variant::TonalSpot => t_max_c(s.primary_palette, 0.0, 90.0, 1.0),
            _ => t_max_c(s.primary_palette, 0.0, 100.0, 1.0),
        })
        .is_background()
        .background(|s| Some(spec::surface_container_high(s)))
        .contrast_curve(|_| Some(get_curve(4.5)))
        .tone_delta_pair(|s| dim_pair(spec::primary_dim(s), spec::primary(s)))
        .build()
}

pub fn on_primary() -> DynamicColor {
    colour("onPrimary", |s| s.primary_palette)
        .background(|s| {
            Some(if s.platform.is_phone() { spec::primary(s) } else { spec::primary_dim(s) })
        })
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(6.0) } else { get_curve(7.0) }))
        .build()
}

pub fn primary_container() -> DynamicColor {
    colour("primaryContainer", |s| s.primary_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                return 30.0;
            }
            match s.variant {
                Variant::Neutral => {
                    if s.is_dark {
                        30.0
                    } else {
                        90.0
                    }
                }
                Variant::TonalSpot => {
                    if s.is_dark {
                        t_min_c(s.primary_palette, 35.0, 93.0)
                    } else {
                        t_max_c(s.primary_palette, 0.0, 90.0, 1.0)
                    }
                }
                Variant::Expressive => {
                    if s.is_dark {
                        t_max_c(s.primary_palette, 30.0, 93.0, 1.0)
                    } else if Hct::is_cyan(s.primary_palette.hue) {
                        t_max_c(s.primary_palette, 78.0, 88.0, 1.0)
                    } else {
                        t_max_c(s.primary_palette, 78.0, 90.0, 1.0)
                    }
                }
                Variant::Vibrant => {
                    if s.is_dark {
                        t_min_c(s.primary_palette, 66.0, 93.0)
                    } else if Hct::is_cyan(s.primary_palette.hue) {
                        t_max_c(s.primary_palette, 66.0, 88.0, 1.0)
                    } else {
                        t_max_c(s.primary_palette, 66.0, 93.0, 1.0)
                    }
                }
                _ => 0.0,
            }
        })
        .is_background()
        .background(container_background)
        .tone_delta_pair(|s| {
            if s.platform.is_phone() {
                None
            } else {
                watch_container_pair(spec::primary_container(s), spec::primary_dim(s))
            }
        })
        .contrast_curve(container_contrast_curve)
        .build()
}

pub fn on_primary_container() -> DynamicColor {
    colour("onPrimaryContainer", |s| s.primary_palette)
        .background(|s| Some(spec::primary_container(s)))
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(6.0) } else { get_curve(7.0) }))
        .build()
}

pub fn primary_fixed() -> DynamicColor {
    colour("primaryFixed", |s| s.primary_palette)
        // Fixed roles are the light-mode container tone, whichever mode the
        // scheme is in — that is what makes them shareable across both.
        .tone(|s| spec::primary_container(s).get_tone(&s.light_normal()))
        .is_background()
        .background(container_background)
        .contrast_curve(container_contrast_curve)
        .build()
}

pub fn primary_fixed_dim() -> DynamicColor {
    colour("primaryFixedDim", |s| s.primary_palette)
        .tone(|s| spec::primary_fixed(s).get_tone(s))
        .is_background()
        .tone_delta_pair(|s| fixed_dim_pair(spec::primary_fixed_dim(s), spec::primary_fixed(s)))
        .build()
}

pub fn on_primary_fixed() -> DynamicColor {
    colour("onPrimaryFixed", |s| s.primary_palette)
        .background(|s| Some(spec::primary_fixed_dim(s)))
        .contrast_curve(|_| Some(get_curve(7.0)))
        .build()
}

pub fn on_primary_fixed_variant() -> DynamicColor {
    colour("onPrimaryFixedVariant", |s| s.primary_palette)
        .background(|s| Some(spec::primary_fixed_dim(s)))
        .contrast_curve(|_| Some(get_curve(4.5)))
        .build()
}

pub fn inverse_primary() -> DynamicColor {
    colour("inversePrimary", |s| s.primary_palette)
        .tone(|s| t_max_c(s.primary_palette, 0.0, 100.0, 1.0))
        .background(|s| Some(spec::inverse_surface(s)))
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(6.0) } else { get_curve(7.0) }))
        .build()
}

// ---- secondary -----------------------------------------------------------

pub fn secondary() -> DynamicColor {
    colour("secondary", |s| s.secondary_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                return if s.variant == Variant::Neutral {
                    90.0
                } else {
                    t_max_c(s.secondary_palette, 0.0, 90.0, 1.0)
                };
            }
            match s.variant {
                Variant::Neutral => {
                    if s.is_dark {
                        t_min_c(s.secondary_palette, 0.0, 98.0)
                    } else {
                        t_max_c(s.secondary_palette, 0.0, 100.0, 1.0)
                    }
                }
                Variant::Vibrant => {
                    if s.is_dark {
                        t_max_c(s.secondary_palette, 0.0, 90.0, 1.0)
                    } else {
                        t_max_c(s.secondary_palette, 0.0, 98.0, 1.0)
                    }
                }
                _ => {
                    if s.is_dark {
                        80.0
                    } else {
                        t_max_c(s.secondary_palette, 0.0, 100.0, 1.0)
                    }
                }
            }
        })
        .is_background()
        .background(on_surface_background)
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(4.5) } else { get_curve(7.0) }))
        .tone_delta_pair(|s| {
            if s.platform.is_phone() {
                accent_pair(spec::secondary_container(s), spec::secondary(s))
            } else {
                None
            }
        })
        .build()
}

pub fn secondary_dim() -> DynamicColor {
    colour("secondaryDim", |s| s.secondary_palette)
        .tone(|s| {
            if s.variant == Variant::Neutral {
                85.0
            } else {
                t_max_c(s.secondary_palette, 0.0, 90.0, 1.0)
            }
        })
        .is_background()
        .background(|s| Some(spec::surface_container_high(s)))
        .contrast_curve(|_| Some(get_curve(4.5)))
        .tone_delta_pair(|s| dim_pair(spec::secondary_dim(s), spec::secondary(s)))
        .build()
}

pub fn on_secondary() -> DynamicColor {
    colour("onSecondary", |s| s.secondary_palette)
        .background(|s| {
            Some(if s.platform.is_phone() { spec::secondary(s) } else { spec::secondary_dim(s) })
        })
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(6.0) } else { get_curve(7.0) }))
        .build()
}

pub fn secondary_container() -> DynamicColor {
    colour("secondaryContainer", |s| s.secondary_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                return 30.0;
            }
            match s.variant {
                Variant::Vibrant => {
                    if s.is_dark {
                        t_min_c(s.secondary_palette, 30.0, 40.0)
                    } else {
                        t_max_c(s.secondary_palette, 84.0, 90.0, 1.0)
                    }
                }
                Variant::Expressive => {
                    if s.is_dark {
                        15.0
                    } else {
                        t_max_c(s.secondary_palette, 90.0, 95.0, 1.0)
                    }
                }
                _ => {
                    if s.is_dark {
                        25.0
                    } else {
                        90.0
                    }
                }
            }
        })
        .is_background()
        .background(container_background)
        .tone_delta_pair(|s| {
            if s.platform.is_phone() {
                None
            } else {
                watch_container_pair(spec::secondary_container(s), spec::secondary_dim(s))
            }
        })
        .contrast_curve(container_contrast_curve)
        .build()
}

pub fn on_secondary_container() -> DynamicColor {
    colour("onSecondaryContainer", |s| s.secondary_palette)
        .background(|s| Some(spec::secondary_container(s)))
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(6.0) } else { get_curve(7.0) }))
        .build()
}

pub fn secondary_fixed() -> DynamicColor {
    colour("secondaryFixed", |s| s.secondary_palette)
        .tone(|s| spec::secondary_container(s).get_tone(&s.light_normal()))
        .is_background()
        .background(container_background)
        .contrast_curve(container_contrast_curve)
        .build()
}

pub fn secondary_fixed_dim() -> DynamicColor {
    colour("secondaryFixedDim", |s| s.secondary_palette)
        .tone(|s| spec::secondary_fixed(s).get_tone(s))
        .is_background()
        .tone_delta_pair(|s| fixed_dim_pair(spec::secondary_fixed_dim(s), spec::secondary_fixed(s)))
        .build()
}

pub fn on_secondary_fixed() -> DynamicColor {
    colour("onSecondaryFixed", |s| s.secondary_palette)
        .background(|s| Some(spec::secondary_fixed_dim(s)))
        .contrast_curve(|_| Some(get_curve(7.0)))
        .build()
}

pub fn on_secondary_fixed_variant() -> DynamicColor {
    colour("onSecondaryFixedVariant", |s| s.secondary_palette)
        .background(|s| Some(spec::secondary_fixed_dim(s)))
        .contrast_curve(|_| Some(get_curve(4.5)))
        .build()
}

// ---- tertiary ------------------------------------------------------------

pub fn tertiary() -> DynamicColor {
    colour("tertiary", |s| s.tertiary_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                return if s.variant == Variant::TonalSpot {
                    t_max_c(s.tertiary_palette, 0.0, 90.0, 1.0)
                } else {
                    t_max_c(s.tertiary_palette, 0.0, 100.0, 1.0)
                };
            }
            if matches!(s.variant, Variant::Expressive | Variant::Vibrant) {
                if Hct::is_cyan(s.tertiary_palette.hue) {
                    t_max_c(s.tertiary_palette, 0.0, 88.0, 1.0)
                } else if s.is_dark {
                    t_max_c(s.tertiary_palette, 0.0, 98.0, 1.0)
                } else {
                    t_max_c(s.tertiary_palette, 0.0, 100.0, 1.0)
                }
            } else if s.is_dark {
                t_max_c(s.tertiary_palette, 0.0, 98.0, 1.0)
            } else {
                t_max_c(s.tertiary_palette, 0.0, 100.0, 1.0)
            }
        })
        .is_background()
        .background(on_surface_background)
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(4.5) } else { get_curve(7.0) }))
        .tone_delta_pair(|s| {
            if s.platform.is_phone() {
                accent_pair(spec::tertiary_container(s), spec::tertiary(s))
            } else {
                None
            }
        })
        .build()
}

pub fn tertiary_dim() -> DynamicColor {
    colour("tertiaryDim", |s| s.tertiary_palette)
        .tone(|s| {
            if s.variant == Variant::TonalSpot {
                t_max_c(s.tertiary_palette, 0.0, 90.0, 1.0)
            } else {
                t_max_c(s.tertiary_palette, 0.0, 100.0, 1.0)
            }
        })
        .is_background()
        .background(|s| Some(spec::surface_container_high(s)))
        .contrast_curve(|_| Some(get_curve(4.5)))
        .tone_delta_pair(|s| dim_pair(spec::tertiary_dim(s), spec::tertiary(s)))
        .build()
}

pub fn on_tertiary() -> DynamicColor {
    colour("onTertiary", |s| s.tertiary_palette)
        .background(|s| {
            Some(if s.platform.is_phone() { spec::tertiary(s) } else { spec::tertiary_dim(s) })
        })
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(6.0) } else { get_curve(7.0) }))
        .build()
}

pub fn tertiary_container() -> DynamicColor {
    colour("tertiaryContainer", |s| s.tertiary_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                return if s.variant == Variant::TonalSpot {
                    t_max_c(s.tertiary_palette, 0.0, 90.0, 1.0)
                } else {
                    t_max_c(s.tertiary_palette, 0.0, 100.0, 1.0)
                };
            }
            match s.variant {
                Variant::Neutral => {
                    if s.is_dark {
                        t_max_c(s.tertiary_palette, 0.0, 93.0, 1.0)
                    } else {
                        t_max_c(s.tertiary_palette, 0.0, 96.0, 1.0)
                    }
                }
                Variant::TonalSpot => {
                    if s.is_dark {
                        t_max_c(s.tertiary_palette, 0.0, 93.0, 1.0)
                    } else {
                        t_max_c(s.tertiary_palette, 0.0, 100.0, 1.0)
                    }
                }
                Variant::Expressive => {
                    if Hct::is_cyan(s.tertiary_palette.hue) {
                        t_max_c(s.tertiary_palette, 75.0, 88.0, 1.0)
                    } else if s.is_dark {
                        t_max_c(s.tertiary_palette, 75.0, 93.0, 1.0)
                    } else {
                        t_max_c(s.tertiary_palette, 75.0, 100.0, 1.0)
                    }
                }
                Variant::Vibrant => {
                    if s.is_dark {
                        t_max_c(s.tertiary_palette, 0.0, 93.0, 1.0)
                    } else {
                        t_max_c(s.tertiary_palette, 72.0, 100.0, 1.0)
                    }
                }
                _ => 0.0,
            }
        })
        .is_background()
        .background(container_background)
        .tone_delta_pair(|s| {
            if s.platform.is_phone() {
                None
            } else {
                watch_container_pair(spec::tertiary_container(s), spec::tertiary_dim(s))
            }
        })
        .contrast_curve(container_contrast_curve)
        .build()
}

pub fn on_tertiary_container() -> DynamicColor {
    colour("onTertiaryContainer", |s| s.tertiary_palette)
        .background(|s| Some(spec::tertiary_container(s)))
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(6.0) } else { get_curve(7.0) }))
        .build()
}

pub fn tertiary_fixed() -> DynamicColor {
    colour("tertiaryFixed", |s| s.tertiary_palette)
        .tone(|s| spec::tertiary_container(s).get_tone(&s.light_normal()))
        .is_background()
        .background(container_background)
        .contrast_curve(container_contrast_curve)
        .build()
}

pub fn tertiary_fixed_dim() -> DynamicColor {
    colour("tertiaryFixedDim", |s| s.tertiary_palette)
        .tone(|s| spec::tertiary_fixed(s).get_tone(s))
        .is_background()
        .tone_delta_pair(|s| fixed_dim_pair(spec::tertiary_fixed_dim(s), spec::tertiary_fixed(s)))
        .build()
}

pub fn on_tertiary_fixed() -> DynamicColor {
    colour("onTertiaryFixed", |s| s.tertiary_palette)
        .background(|s| Some(spec::tertiary_fixed_dim(s)))
        .contrast_curve(|_| Some(get_curve(7.0)))
        .build()
}

pub fn on_tertiary_fixed_variant() -> DynamicColor {
    colour("onTertiaryFixedVariant", |s| s.tertiary_palette)
        .background(|s| Some(spec::tertiary_fixed_dim(s)))
        .contrast_curve(|_| Some(get_curve(4.5)))
        .build()
}

// ---- error ---------------------------------------------------------------

pub fn error() -> DynamicColor {
    colour("error", |s| s.error_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                t_min_c(s.error_palette, 0.0, 100.0)
            } else if s.is_dark {
                t_min_c(s.error_palette, 0.0, 98.0)
            } else {
                t_max_c(s.error_palette, 0.0, 100.0, 1.0)
            }
        })
        .is_background()
        .background(on_surface_background)
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(4.5) } else { get_curve(7.0) }))
        .tone_delta_pair(|s| {
            if s.platform.is_phone() {
                accent_pair(spec::error_container(s), spec::error(s))
            } else {
                None
            }
        })
        .build()
}

pub fn error_dim() -> DynamicColor {
    colour("errorDim", |s| s.error_palette)
        .tone(|s| t_min_c(s.error_palette, 0.0, 100.0))
        .is_background()
        .background(|s| Some(spec::surface_container_high(s)))
        .contrast_curve(|_| Some(get_curve(4.5)))
        .tone_delta_pair(|s| dim_pair(spec::error_dim(s), spec::error(s)))
        .build()
}

pub fn on_error() -> DynamicColor {
    colour("onError", |s| s.error_palette)
        .background(|s| Some(if s.platform.is_phone() { spec::error(s) } else { spec::error_dim(s) }))
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(6.0) } else { get_curve(7.0) }))
        .build()
}

pub fn error_container() -> DynamicColor {
    colour("errorContainer", |s| s.error_palette)
        .tone(|s| {
            if !s.platform.is_phone() {
                30.0
            } else if s.is_dark {
                t_min_c(s.error_palette, 30.0, 93.0)
            } else {
                t_max_c(s.error_palette, 0.0, 90.0, 1.0)
            }
        })
        .is_background()
        .background(container_background)
        .tone_delta_pair(|s| {
            if s.platform.is_phone() {
                None
            } else {
                watch_container_pair(spec::error_container(s), spec::error_dim(s))
            }
        })
        .contrast_curve(container_contrast_curve)
        .build()
}

pub fn on_error_container() -> DynamicColor {
    colour("onErrorContainer", |s| s.error_palette)
        .background(|s| Some(spec::error_container(s)))
        .contrast_curve(|s| Some(if s.platform.is_phone() { get_curve(4.5) } else { get_curve(7.0) }))
        .build()
}

// ---- aliases -------------------------------------------------------------
// 2025 folded four roles into others. They keep their names so existing
// themes do not break, but they are the same colour.

pub fn surface_variant() -> DynamicColor {
    surface_container_highest().renamed("surfaceVariant")
}

pub fn surface_tint() -> DynamicColor {
    primary().renamed("surfaceTint")
}

pub fn background() -> DynamicColor {
    surface().renamed("background")
}

pub fn on_background() -> DynamicColor {
    let mut colour = on_surface().renamed("onBackground");
    colour.tone = std::rc::Rc::new(|s| {
        if s.platform.is_phone() {
            spec::on_surface(s).get_tone(s)
        } else {
            100.0
        }
    });
    colour
}

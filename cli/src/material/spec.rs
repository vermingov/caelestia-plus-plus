//! Which generation of a colour role applies, resolved per scheme.
//!
//! The library builds one wrapper per role that picks the 2021 or the 2025
//! definition when it is asked for, and every cross-reference inside both
//! tables goes through that wrapper — so a 2021 role asking for `surfaceDim`
//! gets the 2025 one under a 2025 scheme. These functions are that wrapper.
//!
//! Three shapes: most roles exist in both tables; the four `*Dim` roles are
//! 2025 additions with no 2021 definition at all, so they are used as-is
//! whichever scheme asks; `shadow`, `scrim` and the six palette key colours
//! were never respecified, so they always come from 2021.

use super::dynamic::DynamicColor;
use super::scheme::DynamicScheme;
use super::variant::SpecVersion;
use super::{spec2021, spec2025};

fn is_2025(s: &DynamicScheme) -> bool {
    s.spec_version == SpecVersion::V2025
}

pub fn background(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::background() } else { spec2021::background() }
}

pub fn on_background(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_background() } else { spec2021::on_background() }
}

pub fn surface(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface() } else { spec2021::surface() }
}

pub fn surface_dim(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface_dim() } else { spec2021::surface_dim() }
}

pub fn surface_bright(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface_bright() } else { spec2021::surface_bright() }
}

pub fn surface_container_lowest(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface_container_lowest() } else { spec2021::surface_container_lowest() }
}

pub fn surface_container_low(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface_container_low() } else { spec2021::surface_container_low() }
}

pub fn surface_container(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface_container() } else { spec2021::surface_container() }
}

pub fn surface_container_high(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface_container_high() } else { spec2021::surface_container_high() }
}

pub fn surface_container_highest(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface_container_highest() } else { spec2021::surface_container_highest() }
}

pub fn on_surface(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_surface() } else { spec2021::on_surface() }
}

pub fn surface_variant(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface_variant() } else { spec2021::surface_variant() }
}

pub fn on_surface_variant(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_surface_variant() } else { spec2021::on_surface_variant() }
}

pub fn outline(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::outline() } else { spec2021::outline() }
}

pub fn outline_variant(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::outline_variant() } else { spec2021::outline_variant() }
}

pub fn inverse_surface(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::inverse_surface() } else { spec2021::inverse_surface() }
}

pub fn inverse_on_surface(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::inverse_on_surface() } else { spec2021::inverse_on_surface() }
}

pub fn shadow(_s: &DynamicScheme) -> DynamicColor {
    spec2021::shadow()
}

pub fn scrim(_s: &DynamicScheme) -> DynamicColor {
    spec2021::scrim()
}

pub fn surface_tint(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::surface_tint() } else { spec2021::surface_tint() }
}

pub fn primary(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::primary() } else { spec2021::primary() }
}

pub fn primary_dim(_s: &DynamicScheme) -> DynamicColor {
    spec2025::primary_dim()
}

pub fn on_primary(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_primary() } else { spec2021::on_primary() }
}

pub fn primary_container(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::primary_container() } else { spec2021::primary_container() }
}

pub fn on_primary_container(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_primary_container() } else { spec2021::on_primary_container() }
}

pub fn inverse_primary(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::inverse_primary() } else { spec2021::inverse_primary() }
}

pub fn primary_fixed(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::primary_fixed() } else { spec2021::primary_fixed() }
}

pub fn primary_fixed_dim(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::primary_fixed_dim() } else { spec2021::primary_fixed_dim() }
}

pub fn on_primary_fixed(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_primary_fixed() } else { spec2021::on_primary_fixed() }
}

pub fn on_primary_fixed_variant(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_primary_fixed_variant() } else { spec2021::on_primary_fixed_variant() }
}

pub fn secondary(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::secondary() } else { spec2021::secondary() }
}

pub fn secondary_dim(_s: &DynamicScheme) -> DynamicColor {
    spec2025::secondary_dim()
}

pub fn on_secondary(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_secondary() } else { spec2021::on_secondary() }
}

pub fn secondary_container(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::secondary_container() } else { spec2021::secondary_container() }
}

pub fn on_secondary_container(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_secondary_container() } else { spec2021::on_secondary_container() }
}

pub fn secondary_fixed(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::secondary_fixed() } else { spec2021::secondary_fixed() }
}

pub fn secondary_fixed_dim(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::secondary_fixed_dim() } else { spec2021::secondary_fixed_dim() }
}

pub fn on_secondary_fixed(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_secondary_fixed() } else { spec2021::on_secondary_fixed() }
}

pub fn on_secondary_fixed_variant(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_secondary_fixed_variant() } else { spec2021::on_secondary_fixed_variant() }
}

pub fn tertiary(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::tertiary() } else { spec2021::tertiary() }
}

pub fn tertiary_dim(_s: &DynamicScheme) -> DynamicColor {
    spec2025::tertiary_dim()
}

pub fn on_tertiary(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_tertiary() } else { spec2021::on_tertiary() }
}

pub fn tertiary_container(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::tertiary_container() } else { spec2021::tertiary_container() }
}

pub fn on_tertiary_container(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_tertiary_container() } else { spec2021::on_tertiary_container() }
}

pub fn tertiary_fixed(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::tertiary_fixed() } else { spec2021::tertiary_fixed() }
}

pub fn tertiary_fixed_dim(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::tertiary_fixed_dim() } else { spec2021::tertiary_fixed_dim() }
}

pub fn on_tertiary_fixed(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_tertiary_fixed() } else { spec2021::on_tertiary_fixed() }
}

pub fn on_tertiary_fixed_variant(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_tertiary_fixed_variant() } else { spec2021::on_tertiary_fixed_variant() }
}

pub fn error(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::error() } else { spec2021::error() }
}

pub fn error_dim(_s: &DynamicScheme) -> DynamicColor {
    spec2025::error_dim()
}

pub fn on_error(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_error() } else { spec2021::on_error() }
}

pub fn error_container(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::error_container() } else { spec2021::error_container() }
}

pub fn on_error_container(s: &DynamicScheme) -> DynamicColor {
    if is_2025(s) { spec2025::on_error_container() } else { spec2021::on_error_container() }
}

pub fn primary_palette_key_colour(_s: &DynamicScheme) -> DynamicColor {
    spec2021::primary_palette_key_colour()
}

pub fn secondary_palette_key_colour(_s: &DynamicScheme) -> DynamicColor {
    spec2021::secondary_palette_key_colour()
}

pub fn tertiary_palette_key_colour(_s: &DynamicScheme) -> DynamicColor {
    spec2021::tertiary_palette_key_colour()
}

pub fn neutral_palette_key_colour(_s: &DynamicScheme) -> DynamicColor {
    spec2021::neutral_palette_key_colour()
}

pub fn neutral_variant_palette_key_colour(_s: &DynamicScheme) -> DynamicColor {
    spec2021::neutral_variant_palette_key_colour()
}

pub fn error_palette_key_colour(_s: &DynamicScheme) -> DynamicColor {
    spec2021::error_palette_key_colour()
}

/// The surface a foreground is measured against. 2025 never overrode it, but
/// the two surfaces it picks between are both dispatched.
pub fn highest_surface(s: &DynamicScheme) -> DynamicColor {
    spec2021::highest_surface(s)
}

/// Every role, in the order Material declares them.
pub const ALL: &[(&str, fn(&DynamicScheme) -> DynamicColor)] = &[

    ("background", background),

    ("onBackground", on_background),

    ("surface", surface),

    ("surfaceDim", surface_dim),

    ("surfaceBright", surface_bright),

    ("surfaceContainerLowest", surface_container_lowest),

    ("surfaceContainerLow", surface_container_low),

    ("surfaceContainer", surface_container),

    ("surfaceContainerHigh", surface_container_high),

    ("surfaceContainerHighest", surface_container_highest),

    ("onSurface", on_surface),

    ("surfaceVariant", surface_variant),

    ("onSurfaceVariant", on_surface_variant),

    ("outline", outline),

    ("outlineVariant", outline_variant),

    ("inverseSurface", inverse_surface),

    ("inverseOnSurface", inverse_on_surface),

    ("shadow", shadow),

    ("scrim", scrim),

    ("surfaceTint", surface_tint),

    ("primary", primary),

    ("primaryDim", primary_dim),

    ("onPrimary", on_primary),

    ("primaryContainer", primary_container),

    ("onPrimaryContainer", on_primary_container),

    ("inversePrimary", inverse_primary),

    ("primaryFixed", primary_fixed),

    ("primaryFixedDim", primary_fixed_dim),

    ("onPrimaryFixed", on_primary_fixed),

    ("onPrimaryFixedVariant", on_primary_fixed_variant),

    ("secondary", secondary),

    ("secondaryDim", secondary_dim),

    ("onSecondary", on_secondary),

    ("secondaryContainer", secondary_container),

    ("onSecondaryContainer", on_secondary_container),

    ("secondaryFixed", secondary_fixed),

    ("secondaryFixedDim", secondary_fixed_dim),

    ("onSecondaryFixed", on_secondary_fixed),

    ("onSecondaryFixedVariant", on_secondary_fixed_variant),

    ("tertiary", tertiary),

    ("tertiaryDim", tertiary_dim),

    ("onTertiary", on_tertiary),

    ("tertiaryContainer", tertiary_container),

    ("onTertiaryContainer", on_tertiary_container),

    ("tertiaryFixed", tertiary_fixed),

    ("tertiaryFixedDim", tertiary_fixed_dim),

    ("onTertiaryFixed", on_tertiary_fixed),

    ("onTertiaryFixedVariant", on_tertiary_fixed_variant),

    ("error", error),

    ("errorDim", error_dim),

    ("onError", on_error),

    ("errorContainer", error_container),

    ("onErrorContainer", on_error_container),

    ("primaryPaletteKeyColor", primary_palette_key_colour),

    ("secondaryPaletteKeyColor", secondary_palette_key_colour),

    ("tertiaryPaletteKeyColor", tertiary_palette_key_colour),

    ("neutralPaletteKeyColor", neutral_palette_key_colour),

    ("neutralVariantPaletteKeyColor", neutral_variant_palette_key_colour),

    ("errorPaletteKeyColor", error_palette_key_colour),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::hct::Hct;
    use crate::material::variant::Variant;
    use redcommon::json::Json;

    /// Every colour of every variant, in both modes, at three contrast levels,
    /// against the library the shell's palettes were generated with.
    #[test]
    fn every_colour_matches_the_library() {
        let vectors = redcommon::json::parse(include_str!("../../tests/colour-vectors.json"))
            .expect("reference vectors are readable");
        let Some(Json::Arr(order)) = vectors.get("order") else { panic!("no colour order") };
        let Some(Json::Arr(rows)) = vectors.get("rows") else { panic!("no scheme rows") };

        let names: Vec<&str> = order
            .iter()
            .map(|n| match n {
                Json::Str(s) => s.as_str(),
                _ => panic!("colour names are strings"),
            })
            .collect();
        assert_eq!(names.len(), ALL.len(), "the library has a colour we do not");
        for (ours, theirs) in ALL.iter().zip(&names) {
            assert_eq!(ours.0, *theirs, "colour order differs from the library");
        }

        let num = |v: Option<&Json>| match v {
            Some(Json::Num(n)) => *n,
            _ => f64::NAN,
        };

        let mut checked = 0usize;
        for row in rows {
            let source = Hct::from_int(num(row.get("source")) as u32);
            let variant_name = row.str_field("variant").expect("variant name");
            let variant = Variant::parse(variant_name).expect("known variant");
            let is_dark = row.bool_field("dark", false);
            let contrast = num(row.get("contrast"));
            let scheme = DynamicScheme::new(source, variant, contrast, is_dark);

            let Some(Json::Arr(expected)) = row.get("colours") else { panic!("no colours") };
            for (i, (name, role)) in ALL.iter().enumerate() {
                let ours = role(&scheme).argb(&scheme);
                let theirs = num(expected.get(i)) as u32;
                assert_eq!(
                    ours,
                    theirs,
                    "{name} of {variant_name} {} at contrast {contrast} from {:08x}: {:08x} vs {:08x}",
                    if is_dark { "dark" } else { "light" },
                    source.to_int(),
                    ours,
                    theirs
                );
                checked += 1;
            }
        }
        assert!(checked > 70_000, "only {checked} colours checked");
    }
}

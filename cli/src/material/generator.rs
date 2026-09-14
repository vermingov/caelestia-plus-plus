//! Caelestia's own layer on top of Material.
//!
//! Material gives 59 roles. The shell also wants sixteen terminal colours, the
//! fourteen Catppuccin accent names, KDE's five semantic colours, and a set of
//! older names kept so existing themes keep working — all pulled toward the
//! wallpaper's hue so the desktop reads as one palette.
//!
//! Ported from the Python `caelestia.utils.material.generator`, including the
//! parts that are surprising: see `neutral` below.

use std::collections::HashMap;

use super::blend;
use super::hct::Hct;
use super::math::{difference_degrees, rotation_direction, sanitize_degrees_double};
use super::scheme::DynamicScheme;
use super::spec;
use super::variant::Variant;

/// The sixteen terminal slots, as Monokai in light mode and Gruvbox in dark,
/// before they are pulled toward the wallpaper.
const LIGHT_TERMINAL: [u32; 16] = [
    0xfdf9f3, 0xff6188, 0xa9dc76, 0xfc9867, 0xffd866, 0xf47fd4, 0x78dce8, 0x333034, 0x121212,
    0xff6188, 0xa9dc76, 0xfc9867, 0xffd866, 0xf47fd4, 0x78dce8, 0x333034,
];

const DARK_TERMINAL: [u32; 16] = [
    0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, 0x928374,
    0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
];

const LIGHT_ACCENTS: [u32; 14] = [
    0xdc8a78, 0xdd7878, 0xea76cb, 0x8839ef, 0xd20f39, 0xe64553, 0xfe640b, 0xdf8e1d, 0x40a02b,
    0x179299, 0x04a5e5, 0x209fb5, 0x1e66f5, 0x7287fd,
];

const DARK_ACCENTS: [u32; 14] = [
    0xf5e0dc, 0xf2cdcd, 0xf5c2e7, 0xcba6f7, 0xf38ba8, 0xeba0ac, 0xfab387, 0xf9e2af, 0xa6e3a1,
    0x94e2d5, 0x89dceb, 0x74c7ec, 0x89b4fa, 0xb4befe,
];

const ACCENT_NAMES: [&str; 14] = [
    "rosewater", "flamingo", "pink", "mauve", "red", "maroon", "peach", "yellow", "green", "teal",
    "sky", "sapphire", "blue", "lavender",
];

/// KDE's semantic colours, which apps read straight out of the scheme.
const KCOLOURS: [(&str, u32); 5] = [
    ("klink", 0x2980b9),
    ("kvisited", 0x9b59b6),
    ("knegative", 0xda4453),
    ("kneutral", 0xf67400),
    ("kpositive", 0x27ae60),
];

fn hct(rgb: u32) -> Hct {
    Hct::from_int(0xff00_0000 | rgb)
}

/// Rotate a colour's hue up to 100 degrees toward another, and lift or drop
/// its tone. This is what keeps a red terminal red while still belonging to a
/// blue wallpaper.
fn harmonise(from: Hct, to: Hct, tone_boost: f64) -> Hct {
    let rotation_degrees = (difference_degrees(from.hue(), to.hue()) * 0.8).min(100.0);
    let output_hue =
        sanitize_degrees_double(from.hue() + rotation_degrees * rotation_direction(from.hue(), to.hue()));
    Hct::from_hct(output_hue, from.chroma(), from.tone() * (1.0 + tone_boost))
}

fn lighten(colour: Hct, amount: f64) -> Hct {
    let diff = (100.0 - colour.tone()) * amount;
    Hct::from_hct(colour.hue(), colour.chroma() + diff / 5.0, colour.tone() + diff)
}

fn darken(colour: Hct, amount: f64) -> Hct {
    let diff = colour.tone() * amount;
    Hct::from_hct(colour.hue(), colour.chroma() - diff / 5.0, colour.tone() - diff)
}

/// Push a colour to one end and strip its chroma — the monochrome variant's
/// answer for colours that are otherwise defined by their hue.
fn greyscale(colour: Hct, light: bool) -> Hct {
    let mut colour = if light { darken(colour, 0.35) } else { lighten(colour, 0.65) };
    colour.set_chroma(0.0);
    colour
}

fn mix(a: Hct, b: Hct, weight: f64) -> Hct {
    Hct::from_int(blend::cam16_ucs(a.to_int(), b.to_int(), weight))
}

/// An insertion-ordered map where two names can share one colour.
///
/// The Python this is ported from keeps colour *objects* in a dict, so the
/// backwards-compatible aliases (`primary_paletteKeyColor` and friends) are
/// the very same object as the name they alias. The `neutral` variant then
/// walks the dict subtracting chroma in place, so an aliased colour has it
/// subtracted once per name pointing at it — twice, not once.
///
/// It happens not to show: every palette a `neutral` scheme builds has less
/// chroma than the 15 being subtracted, so one pass already grounds it out.
/// That is a fact about today's palette numbers, not about the code, so the
/// shape is reproduced rather than simplified away.
struct Palette {
    entries: Vec<(String, usize)>,
    cells: Vec<Hct>,
    index: HashMap<String, usize>,
}

impl Palette {
    fn new() -> Palette {
        Palette { entries: Vec::new(), cells: Vec::new(), index: HashMap::new() }
    }

    /// Bind a name to a new colour. Any other name that shared its previous
    /// colour keeps that one, exactly as rebinding a dict key would.
    fn set(&mut self, name: &str, colour: Hct) {
        self.cells.push(colour);
        let cell = self.cells.len() - 1;
        match self.index.insert(name.to_string(), cell) {
            Some(_) => {
                for entry in self.entries.iter_mut() {
                    if entry.0 == name {
                        entry.1 = cell;
                    }
                }
            }
            None => self.entries.push((name.to_string(), cell)),
        }
    }

    /// Bind a name to the colour another name already has — the same colour,
    /// not a copy.
    fn alias(&mut self, name: &str, existing: &str) {
        let cell = self.index[existing];
        self.index.insert(name.to_string(), cell);
        self.entries.push((name.to_string(), cell));
    }

    fn get(&self, name: &str) -> Hct {
        self.cells[self.index[name]]
    }

    fn names(&self) -> Vec<String> {
        self.entries.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Walk every name in order and change the colour behind it. A colour two
    /// names share is changed twice.
    fn map_in_place(&mut self, f: impl Fn(Hct) -> Hct) {
        for (_, cell) in &self.entries {
            self.cells[*cell] = f(self.cells[*cell]);
        }
    }
}

/// Every colour the shell reads, as lowercase `rrggbb`, in the order the cache
/// files on disk have them.
pub fn gen_scheme(variant: &str, flavour: &str, mode: &str, primary: Hct) -> Vec<(String, String)> {
    let is_light = mode == "light";
    let is_monochrome = variant == "monochrome";
    let parsed = Variant::parse(variant).unwrap_or(Variant::Vibrant);
    let scheme = DynamicScheme::new(primary, parsed, 0.0, !is_light);

    let mut colours = Palette::new();
    for (name, role) in spec::ALL {
        colours.set(name, role(&scheme).hct(&scheme));
    }

    // Older names for the palette key colours, kept so existing themes and
    // templates do not break.
    for name in ["primary", "secondary", "tertiary", "neutral"] {
        colours.alias(&format!("{name}_paletteKeyColor"), &format!("{name}PaletteKeyColor"));
    }
    colours.alias("neutral_variant_paletteKeyColor", "neutralVariantPaletteKeyColor");

    let key = colours.get("primary_paletteKeyColor");

    let terminal = if is_light { LIGHT_TERMINAL } else { DARK_TERMINAL };
    for (i, rgb) in terminal.iter().enumerate() {
        let source = hct(*rgb);
        let colour = if is_monochrome {
            greyscale(source, is_light)
        } else {
            // The first eight are the normal set and move further; the bright
            // eight are nudged, so the pair stays distinguishable.
            let boost = if i < 8 { 0.35 } else { 0.2 };
            harmonise(source, key, boost * if is_light { -1.0 } else { 1.0 })
        };
        colours.set(&format!("term{i}"), colour);
    }

    let accents = if is_light { LIGHT_ACCENTS } else { DARK_ACCENTS };
    for (i, rgb) in accents.iter().enumerate() {
        let source = hct(*rgb);
        let colour = if is_monochrome {
            greyscale(source, is_light)
        } else {
            harmonise(source, key, if is_light { -0.2 } else { 0.05 })
        };
        colours.set(ACCENT_NAMES[i], colour);
    }

    for (name, rgb) in KCOLOURS {
        let selection = format!("{name}Selection");
        colours.set(name, harmonise(hct(rgb), colours.get("primary"), 0.1));
        colours.set(&selection, harmonise(hct(rgb), colours.get("onPrimaryFixedVariant"), 0.1));
        if is_monochrome {
            colours.set(name, greyscale(colours.get(name), is_light));
            colours.set(&selection, greyscale(colours.get(&selection), is_light));
        }
    }

    // The neutral variant is desaturated a second time on top of its palettes,
    // in place — see the note on Palette for why the aliases matter here.
    if variant == "neutral" {
        colours.map_in_place(|mut c| {
            c.set_chroma(c.chroma() - 15.0);
            c
        });
    }

    let hard = flavour == "hard";
    if hard {
        let mut surfaces = vec!["background".to_string()];
        surfaces.extend(colours.names().into_iter().filter(|n| n.starts_with("surface")));
        for name in surfaces {
            let colour = colours.get(&name);
            colours.set(&name, if is_light { lighten(colour, 0.4) } else { darken(colour, 0.8) });
        }
        let term0 = colours.get("term0");
        colours.set("term0", if is_light { lighten(term0, 0.4) } else { darken(term0, 0.9) });
    }

    // Names the shell used before it spoke Material. Kept working.
    colours.alias("text", "onBackground");
    colours.alias("subtext1", "onSurfaceVariant");
    colours.alias("subtext0", "outline");
    let (surface, outline) = (colours.get("surface"), colours.get("outline"));
    for (name, weight) in [
        ("overlay2", 0.86),
        ("overlay1", 0.71),
        ("overlay0", 0.57),
        ("surface2", 0.43),
        ("surface1", 0.29),
        ("surface0", 0.14),
    ] {
        colours.set(name, mix(surface, outline, weight));
    }
    colours.alias("base", "surface");
    colours.set("mantle", darken(surface, 0.03));
    colours.set("crust", darken(surface, 0.05));

    if hard {
        for name in ["base", "mantle", "crust"] {
            let colour = colours.get(name);
            colours.set(name, if is_light { lighten(colour, 0.4) } else { darken(colour, 0.9) });
        }
        for i in 0..3 {
            for name in [format!("overlay{i}"), format!("surface{i}")] {
                let colour = colours.get(&name);
                colours.set(&name, if is_light { lighten(colour, 0.4) } else { darken(colour, 0.8) });
            }
        }
    }

    let mut out: Vec<(String, String)> = colours
        .entries
        .iter()
        .map(|(name, cell)| (name.clone(), format!("{:06x}", colours.cells[*cell].to_int() & 0xff_ffff)))
        .collect();

    // Success has no Material role, so it is a fixed pair either way.
    let extended = if is_light {
        [("success", "4F6354"), ("onSuccess", "FFFFFF"), ("successContainer", "D1E8D5"), ("onSuccessContainer", "0C1F13")]
    } else {
        [("success", "B5CCBA"), ("onSuccess", "213528"), ("successContainer", "374B3E"), ("onSuccessContainer", "D1E9D6")]
    };
    out.extend(extended.iter().map(|(n, v)| (n.to_string(), v.to_string())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use redcommon::json::Json;

    #[test]
    fn schemes_match_the_python_generator() {
        let vectors = redcommon::json::parse(include_str!("../../tests/generator-vectors.json"))
            .expect("reference vectors are readable");
        let Some(Json::Arr(rows)) = vectors.get("rows") else { panic!("no rows") };

        let mut checked = 0usize;
        for row in rows {
            let Some(Json::Num(source)) = row.get("source") else { panic!("no source") };
            let variant = row.str_field("variant").expect("variant");
            let flavour = row.str_field("flavour").expect("flavour");
            let mode = row.str_field("mode").expect("mode");
            let ours = gen_scheme(variant, flavour, mode, Hct::from_int(*source as u32));

            let Some(Json::Arr(expected)) = row.get("colours") else { panic!("no colours") };
            assert_eq!(ours.len(), expected.len(), "{variant}/{flavour}/{mode}: colour count");
            for (i, (name, value)) in ours.iter().enumerate() {
                let Json::Arr(pair) = &expected[i] else { panic!("pairs") };
                let (Json::Str(their_name), Json::Str(their_value)) = (&pair[0], &pair[1]) else {
                    panic!("pairs are strings")
                };
                assert_eq!(name, their_name, "{variant}/{flavour}/{mode}: name at {i}");
                assert_eq!(
                    value, their_value,
                    "{variant}/{flavour}/{mode}: {name} from {:08x}",
                    *source as u32
                );
                checked += 1;
            }
        }
        assert!(checked > 5_000, "only {checked} colours checked");
    }
}

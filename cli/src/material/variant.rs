//! The nine ways a palette can be derived from one colour, and the two
//! generations of the rules that turn those palettes into a scheme.

/// Which recipe builds the palettes. The names are the ones the shell's config
/// uses, minus the underscore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Monochrome,
    Neutral,
    TonalSpot,
    Vibrant,
    Expressive,
    Fidelity,
    Content,
    Rainbow,
    FruitSalad,
}

impl Variant {
    pub fn parse(name: &str) -> Option<Variant> {
        Some(match name {
            "monochrome" => Variant::Monochrome,
            "neutral" => Variant::Neutral,
            "tonalspot" => Variant::TonalSpot,
            "vibrant" => Variant::Vibrant,
            "expressive" => Variant::Expressive,
            "fidelity" => Variant::Fidelity,
            "content" => Variant::Content,
            "rainbow" => Variant::Rainbow,
            "fruitsalad" => Variant::FruitSalad,
            _ => return None,
        })
    }
}

/// Which generation of the colour rules applies.
///
/// Only four of the variants were reworked for 2025; the rest still run the
/// 2021 rules, and the library falls them back automatically. The shell's
/// palettes on disk were generated under exactly this split, so it is not a
/// detail that can be simplified away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecVersion {
    V2021,
    V2025,
}

impl SpecVersion {
    /// 2025 exists only for the four variants that were respecified. Asking for
    /// it with any other variant quietly gets you 2021.
    pub fn for_variant(requested: SpecVersion, variant: Variant) -> SpecVersion {
        match variant {
            Variant::Expressive | Variant::Vibrant | Variant::TonalSpot | Variant::Neutral => requested,
            _ => SpecVersion::V2021,
        }
    }
}

/// Screen size the scheme is for. The shell only ever draws on a phone-sized
/// spec — "watch" exists because the 2025 rules branch on it throughout, and
/// dropping the branches would make this port impossible to diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Phone,
    Watch,
}

impl Platform {
    pub fn is_phone(self) -> bool {
        self == Platform::Phone
    }
}

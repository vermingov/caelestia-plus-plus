//! The nine Material palette variants, with what each one does to a scheme.
//!
//! The same list and the same descriptions the shell shows, so the two agree
//! about what "expressive" means.

pub struct Variant {
    pub id: &'static str,
    pub icon: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub const ALL: [Variant; 9] = [
    Variant {
        id: "vibrant",
        icon: "sentiment_very_dissatisfied",
        name: "Vibrant",
        description: "A high chroma palette. The primary palette's chroma is at maximum.",
    },
    Variant {
        id: "tonalspot",
        icon: "android",
        name: "Tonal Spot",
        description: "Default for Material theme colours. A pastel palette with a low chroma.",
    },
    Variant {
        id: "expressive",
        icon: "compare_arrows",
        name: "Expressive",
        description: "A medium chroma palette. The primary palette's hue is different from the seed colour, for variety.",
    },
    Variant {
        id: "fidelity",
        icon: "compare",
        name: "Fidelity",
        description: "Matches the seed colour, even if the seed colour is very bright (high chroma).",
    },
    Variant {
        id: "content",
        icon: "sentiment_calm",
        name: "Content",
        description: "Almost identical to fidelity.",
    },
    Variant {
        id: "fruitsalad",
        icon: "nutrition",
        name: "Fruit Salad",
        description: "A playful theme - the seed colour's hue does not appear in the theme.",
    },
    Variant {
        id: "rainbow",
        icon: "looks",
        name: "Rainbow",
        description: "A playful theme - the seed colour's hue does not appear in the theme.",
    },
    Variant {
        id: "neutral",
        icon: "contrast",
        name: "Neutral",
        description: "Close to grayscale, a hint of chroma.",
    },
    Variant {
        id: "monochrome",
        icon: "filter_b_and_w",
        name: "Monochrome",
        description: "All colours are grayscale, no chroma.",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_the_cli_accepts_is_offered() {
        // The set the scheme generator implements; missing one means a
        // palette the user cannot reach from the launcher.
        let expected = [
            "vibrant", "tonalspot", "expressive", "fidelity", "content", "fruitsalad", "rainbow",
            "neutral", "monochrome",
        ];
        let mut ours: Vec<&str> = ALL.iter().map(|v| v.id).collect();
        let mut theirs = expected.to_vec();
        ours.sort_unstable();
        theirs.sort_unstable();
        assert_eq!(ours, theirs);
    }
}

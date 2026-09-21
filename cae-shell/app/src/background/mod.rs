//! What is behind everything: the wallpaper, or where the settings ask for
//! none, the helix, in the scheme's colour or one of the user's own. Drawn by
//! the `easel`, and Quickshell's to draw until Quickshell says it has stopped,
//! like everything else taken from it.

mod clock;
mod easel;

use std::path::PathBuf;
use std::time::Duration;

use cae_core::launcher::wallpapers;
use cae_core::{config, scheme};
use gpui::{App, AppContext, AsyncApp, Context};

use crate::feeds::Feeds;
use crate::ours;

const PIECE: &str = "background";

/// The mark's own red, which is what the helix was first drawn in.
const RED: [f32; 3] = [1., 0.329, 0.286];

/// Thirty frames a second, and fifteen on the battery: it is a slow thing,
/// and half as many frames is half the work.
const ON_MAINS: Duration = Duration::from_millis(33);
const ON_BATTERY: Duration = Duration::from_millis(66);

/// What the settings ask to have behind everything, which may be nothing.
///
/// A wallpaper that is asked for and is not there is not a reason for a bare
/// desktop: the helix stands in for it, as it does for one that was never
/// asked for.
fn asked_for() -> Option<easel::Showing> {
    let files = config::Snapshot::read();
    let flag = |root: &serde_json::Value, path: &str, otherwise: bool| {
        config::lookup(root, path).and_then(serde_json::Value::as_bool).unwrap_or(otherwise)
    };
    if !flag(&files.shell, "background.enabled", true) {
        return None;
    }
    let wallpaper = wallpapers::current().map(PathBuf::from).filter(|picture| picture.is_file());
    if let Some(picture) = wallpaper.filter(|_| flag(&files.shell, "background.wallpaperEnabled", true)) {
        return Some(easel::Showing::Picture(picture));
    }
    if !flag(&files.prefs, "dnaEnabled", true) {
        return None;
    }
    let own = config::lookup(&files.prefs, "dnaCustomColor").and_then(serde_json::Value::as_str).and_then(rgb);
    let accent = if flag(&files.prefs, "dnaUseThemeColor", true) { scheme::primary().and_then(|hex| rgb(&hex)) } else { own };
    Some(easel::Showing::Helix(shades(accent.unwrap_or(RED))))
}

/// "#ff5449" or "ff5449", as red, green and blue from nought to one.
fn rgb(hex: &str) -> Option<[f32; 3]> {
    let digits = hex.trim_start_matches('#');
    if digits.len() != 6 {
        return None;
    }
    let part = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok().map(|value| f32::from(value) / 255.);
    Some([part(0)?, part(2)?, part(4)?])
}

fn hsv([red, green, blue]: [f32; 3]) -> [f32; 3] {
    let (most, least) = (red.max(green).max(blue), red.min(green).min(blue));
    let spread = most - least;
    let hue = if spread == 0. {
        0.
    } else if most == red {
        ((green - blue) / spread).rem_euclid(6.)
    } else if most == green {
        (blue - red) / spread + 2.
    } else {
        (red - green) / spread + 4.
    };
    [hue / 6., if most == 0. { 0. } else { spread / most }, most]
}

fn from_hsv([hue, saturation, value]: [f32; 3]) -> [f32; 3] {
    let sector = hue.rem_euclid(1.) * 6.;
    let chroma = value * saturation;
    let other = chroma * (1. - (sector % 2. - 1.).abs());
    let [red, green, blue] = match sector as u32 {
        0 => [chroma, other, 0.],
        1 => [other, chroma, 0.],
        2 => [0., chroma, other],
        3 => [0., other, chroma],
        4 => [other, 0., chroma],
        _ => [chroma, 0., other],
    };
    let lift = value - chroma;
    [red + lift, green + lift, blue + lift]
}

/// The accent, and a deep and a hot shade of it: whatever the hue, the
/// contrast of the red palette it was first drawn in (93000a, ff5449, ffc4b8),
/// which this gives back to within a shade.
fn shades(accent: [f32; 3]) -> easel::Colours {
    let [hue, saturation, value] = hsv(accent);
    easel::Colours {
        deep: from_hsv([hue, (saturation * 1.4).min(1.), value * 0.58]),
        primary: accent,
        hot: from_hsv([hue, saturation * 0.39, value]),
    }
}

struct Background {
    feeds: Feeds,
    asked_for: Option<easel::Showing>,
    easel: Option<easel::Easel>,
    wished: Option<easel::Wishes>,
    /// The clock on top of it, which is off unless asked for.
    clocks: gpui::Entity<clock::Clocks>,
}

impl Background {
    /// Starts, stops or re-tells the easel, so that it shows what the
    /// settings and the desktop ask for now.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let ours = ours::is_ours(PIECE, cx);
        self.clocks.update(cx, |clocks, cx| clocks.follow(ours, cx));
        let Some(showing) = self.asked_for.clone().filter(|_| ours) else {
            (self.easel, self.wished) = (None, None);
            return;
        };
        let on_battery = self.feeds.system.read(cx).value.battery.as_ref().is_some_and(|battery| !battery.on_mains);
        let wishes = easel::Wishes {
            showing,
            seen: self.feeds.hypr.read(cx).value.desktops.clone(),
            frame: if on_battery { ON_BATTERY } else { ON_MAINS },
        };
        if self.wished.as_ref() == Some(&wishes) {
            return;
        }
        match &self.easel {
            Some(easel) => easel.wish(wishes.clone()),
            None => self.easel = easel::Easel::start(wishes.clone()).map_err(|error| eprintln!("cae: cannot start the background: {error}")).ok(),
        }
        self.wished = Some(wishes);
    }
}

/// Keeps the desktop's background for the life of the shell.
pub fn keep(cx: &mut App, feeds: &Feeds) {
    let background = cx.new(|cx| {
        cx.observe(&feeds.hypr, |background: &mut Background, _, cx| background.follow(cx)).detach();
        cx.observe(&feeds.system, |background: &mut Background, _, cx| background.follow(cx)).detach();
        cx.observe(&feeds.settings, |background: &mut Background, _, cx| {
            background.asked_for = asked_for();
            background.follow(cx);
        })
        .detach();
        Background { feeds: feeds.clone(), asked_for: asked_for(), easel: None, wished: None, clocks: clock::keep(cx, feeds) }
    });

    // Quickshell is asked again every so often whether the background is
    // still its own, and nothing above happens on a quiet desktop to ask it.
    // The loop holds the entity, which is what keeps it.
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            background.update(cx, |background, cx| background.follow(cx));
            cx.background_executor().timer(Duration::from_secs(5)).await;
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex([red, green, blue]: [f32; 3]) -> String {
        let byte = |part: f32| (part * 255.).round() as u8;
        format!("{:02x}{:02x}{:02x}", byte(red), byte(green), byte(blue))
    }

    #[test]
    fn a_colour_is_read_with_or_without_its_hash() {
        assert_eq!(rgb("#ff5449").map(hex).as_deref(), Some("ff5449"));
        assert_eq!(rgb("00ff80").map(hex).as_deref(), Some("00ff80"));
        assert_eq!(rgb("#fff"), None);
        assert_eq!(rgb("not one"), None);
    }

    #[test]
    fn the_first_red_gives_the_palette_it_always_had() {
        let shades = shades(rgb("ff5449").unwrap());
        assert_eq!(hex(shades.primary), "ff5449");
        assert_eq!(hex(shades.deep), "940900");
        assert_eq!(hex(shades.hot), "ffbcb8");
    }

    #[test]
    fn a_colour_survives_the_trip_through_hue_and_back() {
        for colour in ["ff5449", "3a7bd5", "00c853", "808080", "000000", "ffffff", "c000ff"] {
            assert_eq!(hex(from_hsv(hsv(rgb(colour).unwrap()))), colour);
        }
    }
}

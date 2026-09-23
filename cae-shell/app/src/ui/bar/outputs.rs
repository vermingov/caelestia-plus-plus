//! A bar on every output, for as long as the output is there.
//!
//! Outputs come and go: a laptop is docked, a projector is plugged in. Each
//! needs a bar of its own, and a bar whose output has gone has to go with it.

use std::collections::HashMap;

use gpui::{AnyWindowHandle, App, DisplayId, Entity};

use crate::feeds::Feeds;
use crate::ui::screen::{self, Screens};

/// Brings the bars into line with the outputs: opens what is missing, closes
/// what has lost its screen.
fn sync(bars: &mut HashMap<DisplayId, AnyWindowHandle>, feeds: &Feeds, preview: bool, cx: &mut App) {
    let wanted = screen::outputs(cx);

    bars.retain(|display, window| {
        let stays = wanted.iter().any(|(_, wanted)| wanted == display);
        if !stays {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
        stays
    });

    for (output, display) in wanted {
        if bars.contains_key(&display) {
            continue;
        }
        if let Some(window) = super::open(cx, display, output, feeds, preview) {
            bars.insert(display, window);
        }
    }
}

/// Keeps a bar on every output for the life of the shell, following the
/// list of outputs as it changes.
pub fn keep(cx: &mut App, screens: &Entity<Screens>, feeds: &Feeds, preview: bool) {
    let feeds = feeds.clone();
    let mut bars = HashMap::new();
    sync(&mut bars, &feeds, preview, cx);
    cx.observe(screens, move |_, cx| sync(&mut bars, &feeds, preview, cx)).detach();
}

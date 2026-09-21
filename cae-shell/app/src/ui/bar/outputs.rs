//! A bar on every output, for as long as the output is there.
//!
//! Outputs come and go: a laptop is docked, a projector is plugged in. Each
//! needs a bar of its own, and a bar whose output has gone has to go with it.

use std::collections::HashMap;
use std::time::Duration;

use gpui::{AnyWindowHandle, App, AsyncApp, DisplayId};

use crate::feeds::Feeds;
use crate::ui::screen;

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

/// Keeps a bar on every output for the life of the shell.
///
/// Checked on a slow tick rather than on an event: GPUI does not say when its
/// displays change, and an output appearing is not something anybody waits
/// on to the millisecond. The first passes are quick, because at startup the
/// displays arrive a few milliseconds after the application does.
pub fn keep_on_every_output(cx: &mut App, feeds: &Feeds, preview: bool) {
    let feeds = feeds.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut bars = HashMap::new();
        let mut passes = 0_u32;
        loop {
            cx.update(|cx| sync(&mut bars, &feeds, preview, cx));
            passes += 1;
            let wait = if bars.is_empty() && passes < 200 { 25 } else { 2000 };
            cx.background_executor().timer(Duration::from_millis(wait)).await;
        }
    })
    .detach();
}

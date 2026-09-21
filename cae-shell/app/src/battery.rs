//! Warning about the battery, and hibernating before the machine dies.
//!
//! The sampler behind the bar already reads the charge every second; this
//! watches those readings go past and says something on the way down. What
//! is worth saying, and when, is `cae_core::battery`, which knows nothing
//! about windows — all that is here is the feed, the handover and the wait
//! before the machine goes down.

use std::time::Duration;

use cae_core::battery::{Say, Settings, Watch};
use cae_core::{session, tell};
use gpui::{App, AppContext, Context, Entity, Global};

use crate::feeds::Feeds;
use crate::ours;

/// What Quickshell is asked about before a word is said: two shells warning
/// about the same battery is one warning too many.
const PIECE: &str = "battery";

/// Between being told the machine is going down and it going down. Long
/// enough to reach for a charger, short enough to matter at three percent.
const GRACE: Duration = Duration::from_secs(5);

struct Watcher {
    feeds: Feeds,
    settings: Settings,
    watch: Watch,
}

/// Held for the life of the shell, which is what keeps the observers below
/// alive.
struct Kept(#[allow(dead_code)] Entity<Watcher>);

impl Global for Kept {}

/// Starts watching.
pub fn keep(cx: &mut App, feeds: &Feeds) {
    let watcher = cx.new(|cx: &mut Context<Watcher>| {
        cx.observe(&feeds.system, |watcher: &mut Watcher, _, cx| watcher.read(cx)).detach();
        cx.observe(&feeds.settings, |watcher: &mut Watcher, _, _| {
            watcher.settings = Settings::read();
        })
        .detach();
        Watcher { feeds: feeds.clone(), settings: Settings::read(), watch: Watch::default() }
    });
    cx.set_global(Kept(watcher));
}

impl Watcher {
    fn read(&mut self, cx: &mut Context<Self>) {
        if !ours::is_ours(PIECE, cx) {
            return;
        }
        let battery = self.feeds.system.read(cx).value.battery.clone();
        for say in self.watch.reading(battery.as_ref(), &self.settings) {
            match say {
                Say::Warn(level) => {
                    let how = if level.critical { tell::How::Warned } else { tell::How::Said };
                    cx.background_spawn(async move { tell::tell(&level.title, &level.message, &level.glyph, how) }).detach();
                }
                Say::Unplugged => {
                    cx.background_spawn(async { tell::said("Charger unplugged", "Battery is discharging", "power_off") }).detach();
                }
                Say::Plugged => {
                    cx.background_spawn(async { tell::said("Charger plugged in", "Battery is charging", "power") }).detach();
                }
                Say::Hibernate => self.go_down(cx),
            }
        }
    }

    /// Says it, waits, and then — only if a charger has not turned up in the
    /// meantime — puts the machine away.
    fn go_down(&mut self, cx: &mut Context<Self>) {
        cx.background_spawn(async {
            tell::tell("Hibernating in 5 seconds", "Hibernating to prevent data loss", "battery_android_alert", tell::How::Warned)
        })
        .detach();
        cx.spawn(async move |watcher, cx| {
            cx.background_executor().timer(GRACE).await;
            let still = watcher.read_with(cx, |watcher: &Watcher, _| watcher.watch.still_going_down()).unwrap_or(false);
            if still {
                cx.background_spawn(async { session::run(&["hibernate".to_string()]) }).await;
            }
        })
        .detach();
    }
}

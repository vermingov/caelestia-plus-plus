//! The panels that only say something: nothing in them is asked for, and
//! nothing in them can be pressed.

use gpui::{Context, IntoElement, Render, Window, prelude::*};

use super::pieces::{column, detail, entry, headline, list, meter, trailing};
use crate::clock;
use crate::feeds::Feeds;
use crate::ui::rsx;

/// Which of them.
#[derive(Clone, Copy)]
pub enum Says {
    Cpu,
    Memory,
    Gpu,
    Active,
    Guards,
    Features,
    Clock,
}

pub struct Simple {
    says: Says,
    feeds: Feeds,
}

impl Simple {
    pub fn new(says: Says, feeds: &Feeds, cx: &mut Context<Self>) -> Simple {
        // Open for seconds, and saying what the bar is saying in more words,
        // so it keeps up with the same feeds.
        cx.observe(&feeds.system, |_, _, cx| cx.notify()).detach();
        cx.observe(&feeds.services, |_, _, cx| cx.notify()).detach();
        cx.observe(&feeds.hypr, |_, _, cx| cx.notify()).detach();
        Simple { says, feeds: feeds.clone() }
    }
}

/// A name, how much of it is in use as a bar, and the same in words.
fn load(name: &'static str, percent: f64, words: String) -> gpui::Div {
    rsx! {
        <div base={column()}>
            {headline(name)}
            {meter(percent)}
            {detail(words)}
        </div>
    }
}

/// What a mode is called, where the bar only has room for a wrench.
pub fn feature_name(id: &str) -> &str {
    match id {
        "maxPerf" => "Maximum performance",
        "antiHeat" => "Anti-Heat",
        "lidStay" => "Stay awake on lid close",
        "caffeine" => "Caffeine",
        "gameMode" => "Game mode",
        "bedMode" => "Bed mode",
        other => other,
    }
}

impl Render for Simple {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let system = &self.feeds.system.read(cx).value;
        let services = &self.feeds.services.read(cx).value;

        match self.says {
            Says::Cpu => load("Processor", system.cpu, format!("{}% in use", system.cpu.round() as i64)),
            Says::Gpu => {
                let busy = system.gpu.unwrap_or_default();
                load("Graphics", busy, format!("{}% in use", busy.round() as i64))
            }
            Says::Memory => load(
                "Memory",
                system.memory,
                format!("{:.1} of {:.1} GiB", system.memory_used_gb, system.memory_total_gb),
            ),
            Says::Active => {
                let active = &self.feeds.hypr.read(cx).value.active;
                let name = if active.class.is_empty() { "Desktop" } else { active.class.as_str() };
                let title = if active.title.is_empty() { "Nothing focused" } else { active.title.as_str() };
                rsx! {
                    <div base={column()}>
                        {headline(name.to_string())}
                        {detail(title.to_string())}
                    </div>
                }
            }
            Says::Guards => {
                let guards = &services.guards;
                let state = match (guards.connected, guards.pending) {
                    (false, _) => "Neither daemon is answering".to_string(),
                    (true, pending) if pending > 0 => format!("{pending} waiting on you"),
                    (true, _) => format!("{} rules, nothing waiting", guards.rules),
                };
                rsx! {
                    <div base={column()}>
                        {headline(if guards.connected { "Protected" } else { "Guards offline" })}
                        {detail(state)}
                        {detail("Click to open the security centre")}
                    </div>
                }
            }
            Says::Features => rsx! {
                <div base={column()}>
                    {headline("Feature modes")}
                    <div base={list()} id="modes" class="overflow-y-scroll">
                        {for feature in services.features.iter() {
                            <div base={entry("tune", feature_name(&feature.id).to_string(), feature.enabled)}>
                                {trailing(if feature.enabled { "On" } else { "Off" })}
                            </div>
                        }}
                    </div>
                    {detail("Click to open the menu")}
                </div>
            },
            // Spelled out in full here, because the bar itself only has room
            // for the short form.
            Says::Clock => rsx! {
                <div base={column()}>
                    {headline(clock::long_date())}
                    {detail(format!("Week {}", clock::week()))}
                </div>
            },
        }
    }
}

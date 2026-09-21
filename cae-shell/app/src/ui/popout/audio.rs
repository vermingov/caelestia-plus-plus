//! Volume and the microphone: a level to drag, and which device it is the
//! level of. The two panels are the same panel about different ends of the
//! same wire.

use std::time::Duration;

use cae_core::{services, volume};
use gpui::{AppContext, Context, IntoElement, Render, Window, prelude::*};

use super::pieces::{caption, column, detail, entry, headline, list, wide};
use crate::feeds::Feeds;
use crate::ui::rsx;
use crate::ui::slider::{Slide, slider};

#[derive(Clone, Copy, PartialEq)]
pub enum End {
    Output,
    Input,
}

pub struct Audio {
    end: End,
    feeds: Feeds,
    /// The devices at this end. Asked for when the panel opens rather than
    /// kept: it costs a process, and is only worth having while looked at.
    nodes: Vec<services::AudioNode>,
    slide: Slide,
}

impl Audio {
    pub fn new(end: End, feeds: &Feeds, cx: &mut Context<Self>) -> Audio {
        cx.observe(&feeds.system, |_, _, cx| cx.notify()).detach();
        let mut audio = Audio { end, feeds: feeds.clone(), nodes: Vec::new(), slide: Slide::default() };
        audio.look(Duration::ZERO, cx);
        audio
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        let end = self.end;
        cx.spawn(async move |audio, cx| {
            cx.background_executor().timer(after).await;
            let nodes = cx
                .background_spawn(async move { if end == End::Output { services::sinks() } else { services::sources() } })
                .await;
            let _ = audio.update(cx, |audio, cx| {
                audio.nodes = nodes;
                cx.notify();
            });
        })
        .detach();
    }

    fn choose(&mut self, name: String, cx: &mut Context<Self>) {
        let kind = if self.end == End::Output { "sink" } else { "source" };
        cx.background_spawn(async move { services::set_default_node(kind, &name) }).detach();
        self.look(Duration::from_millis(400), cx);
    }
}

impl Render for Audio {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let system = &self.feeds.system.read(cx).value;
        let output = self.end == End::Output;
        let Some(level) = (if output { system.volume.clone() } else { system.microphone.clone() }) else {
            return rsx! { <div base={column()}>{headline(if output { "No output" } else { "No microphone" })}</div> };
        };

        let saying = self.slide.showing(level.level);
        let title = match (output, level.muted) {
            (true, true) => "Muted".to_string(),
            (true, false) => format!("Volume {saying}%"),
            (false, true) => "Microphone muted".to_string(),
            (false, false) => format!("Microphone {saying}%"),
        };
        let set = move |value: i64, cx: &mut gpui::App| {
            cx.background_spawn(async move { if output { volume::set(value, 100) } else { volume::set_microphone(value) } })
                .detach();
        };

        rsx! {
            <div base={column()}>
                {headline(title)}
                {slider(&self.slide, level.level, (0, 100), set)}
                {caption(if output { "Output device" } else { "Input device" })}
                <div base={list()} id="nodes" class="overflow-y-scroll">
                    {for (index, node) in self.nodes.iter().enumerate() {
                        <div
                            base={entry(if output { "speaker" } else { "mic" }, node.description.clone(), node.default)}
                            id={("node", index)}
                            onClick={cx.listener({
                                let name = node.name.clone();
                                move |audio, _, _, cx| audio.choose(name.clone(), cx)
                            })}
                        />
                    }}
                </div>
                {detail(format!("Click the icon to {}", if level.muted { "unmute" } else { "mute" }))}
                {...output.then(|| rsx! {
                    <div base={wide("settings", "Open settings")} id="settings" onClick={|_, _, cx| super::open_settings("audio", cx)} />
                })}
            </div>
        }
    }
}

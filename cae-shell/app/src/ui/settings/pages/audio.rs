//! Sound: how loud each end is, which device each end is, and how loud each
//! program is on its way there.

use std::time::Duration;

use cae_core::{services, streams, volume};
use gpui::{AnyElement, AppContext, Context, IntoElement, Render, Window, div, prelude::*, px};

use super::super::Page;
use super::super::frame::Reach;
use super::super::rows::{chosen_mark, leads, level, nothing, page, pick, press, rule, section_title};
use super::super::store::shell;
use crate::ui::rsx;
use crate::ui::slider::{Slide, slider};

/// How long a change takes to show in what PipeWire reports.
const SETTLED: Duration = Duration::from_millis(350);
/// How often the streams are read while their page is open: programs start
/// and stop playing without telling anybody here.
const LOOK: Duration = Duration::from_secs(2);

fn speaker(level: i64, muted: bool) -> &'static str {
    match (muted, level) {
        (true, _) | (_, 0) => "volume_off",
        (_, 1..=40) => "volume_down",
        _ => "volume_up",
    }
}

pub struct Audio {
    reach: Reach,
    sinks: Vec<services::AudioNode>,
    sources: Vec<services::AudioNode>,
    playing: usize,
    output: Slide,
    input: Slide,
}

impl Audio {
    pub fn new(reach: &Reach, cx: &mut Context<Self>) -> Audio {
        cx.observe(&reach.feeds.system, |_, _, cx| cx.notify()).detach();
        cx.observe(&reach.store, |_, _, cx| cx.notify()).detach();
        let mut audio = Audio {
            reach: reach.clone(),
            sinks: Vec::new(),
            sources: Vec::new(),
            playing: 0,
            output: Slide::default(),
            input: Slide::default(),
        };
        audio.look(Duration::ZERO, cx);
        audio
    }

    fn look(&mut self, after: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |audio, cx| {
            cx.background_executor().timer(after).await;
            let found = cx.background_spawn(async { (services::sinks(), services::sources(), streams::list().len()) }).await;
            let _ = audio.update(cx, |audio, cx| {
                (audio.sinks, audio.sources, audio.playing) = found;
                cx.notify();
            });
        })
        .detach();
    }

    fn choose(&mut self, kind: &'static str, name: String, cx: &mut Context<Self>) {
        cx.background_spawn(async move { services::set_default_node(kind, &name) }).detach();
        self.look(SETTLED, cx);
    }

    /// One end of the wire: its level, and the devices it could be.
    fn end(&self, output: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let system = &self.reach.feeds.system.read(cx).value;
        let (now, slide, nodes, kind) = if output {
            (system.volume.clone(), &self.output, &self.sinks, "sink")
        } else {
            (system.microphone.clone(), &self.input, &self.sources, "source")
        };
        let title = if output { "Output" } else { "Input" };
        let Some(now) = now else {
            return vec![
                section_title(title, output).into_any_element(),
                nothing(if output { "speaker" } else { "mic_off" }, if output { "No output device" } else { "No microphone" })
                    .into_any_element(),
            ];
        };

        // As loud as the shell lets the volume keys go, which is the same
        // promise: a slider that stopped short of them would be the odd one.
        let loudest = if output { (self.reach.store.read(cx).number(shell("services.maxVolume"), 1.) * 100.).round() as i64 } else { 100 };
        let symbol = if output { speaker(now.level, now.muted) } else if now.muted { "mic_off" } else { "mic" };
        let mute = rsx! {
            <div
                base={press(symbol, now.muted)}
                id={if output { "mute-output" } else { "mute-input" }}
                onClick={move |_, _, cx| {
                    cx.background_spawn(async move { if output { volume::toggle_mute() } else { volume::toggle_microphone() } }).detach();
                }}
            />
        };
        let says = if now.muted { "Muted".to_string() } else { format!("{}%", slide.showing(now.level)) };
        let set = move |value: i64, cx: &mut gpui::App| {
            cx.background_spawn(async move { if output { volume::set(value, loudest) } else { volume::set_microphone(value) } }).detach();
        };

        let mut rows = vec![
            section_title(title, output).into_any_element(),
            level(mute, if output { "Volume" } else { "Microphone" }, says).into_any_element(),
            rsx! { <div class="flex-none pb-[10px]" when={(now.muted, |track| track.opacity(0.45))}>{slider(slide, now.level, (0, loudest), set)}</div> }
                .into_any_element(),
        ];
        for (index, node) in nodes.iter().enumerate() {
            rows.push(rule().into_any_element());
            let name = node.name.clone();
            rows.push(
                rsx! {
                    <div
                        base={pick(if output { "speaker" } else { "mic" }, node.description.clone(), "", node.default)}
                        id={(kind, index)}
                        onClick={cx.listener(move |audio, _, _, cx| audio.choose(kind, name.clone(), cx))}
                    >
                        {...node.default.then(chosen_mark)}
                    </div>
                }
                .into_any_element(),
            );
        }
        rows
    }
}

impl Render for Audio {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let playing = match self.playing {
            0 => "Nothing is playing".to_string(),
            1 => "1 app is playing".to_string(),
            count => format!("{count} apps are playing"),
        };
        let nav = self.reach.nav.clone();
        rsx! {
            <div base={page()}>
                {...self.end(true, cx)}
                {...self.end(false, cx)}
                <div class="flex-none h-[22px]" />
                <div
                    base={leads("tune", "App volumes", playing)}
                    id="apps"
                    onClick={move |_, window, cx| nav.go(Page::AppVolumes, window, cx)}
                />
            </div>
        }
    }
}

/// Each program that is playing, with a level of its own.
pub struct AppVolumes {
    streams: Vec<streams::Stream>,
    /// A slider's memory for each stream, by PipeWire's number for it.
    slides: Vec<(u32, Slide)>,
}

impl AppVolumes {
    pub fn new(cx: &mut Context<Self>) -> AppVolumes {
        cx.spawn(async move |page, cx| {
            loop {
                let found = cx.background_spawn(async { streams::list() }).await;
                let looked = page.update(cx, |page: &mut AppVolumes, cx| {
                    if page.streams != found {
                        page.slides.retain(|(index, _)| found.iter().any(|stream| stream.index == *index));
                        for stream in &found {
                            if !page.slides.iter().any(|(index, _)| *index == stream.index) {
                                page.slides.push((stream.index, Slide::default()));
                            }
                        }
                        page.streams = found;
                        cx.notify();
                    }
                });
                if looked.is_err() {
                    break;
                }
                cx.background_executor().timer(LOOK).await;
            }
        })
        .detach();
        AppVolumes { streams: Vec::new(), slides: Vec::new() }
    }
}

impl AppVolumes {
    /// Shown at once and asked for after: the listing is only read again
    /// every couple of seconds.
    fn mute(&mut self, index: u32, muted: bool, cx: &mut Context<Self>) {
        if let Some(stream) = self.streams.iter_mut().find(|stream| stream.index == index) {
            stream.muted = muted;
        }
        cx.background_spawn(async move { streams::set_muted(index, muted) }).detach();
        cx.notify();
    }
}

impl Render for AppVolumes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.streams.is_empty() {
            return rsx! { <div base={page()}>{nothing("music_off", "Nothing is playing")}</div> };
        }
        rsx! {
            <div base={page()}>
                {for (place, stream) in self.streams.iter().enumerate() {
                    <div class="flex flex-col flex-none" key={stream.index as usize}>
                        {...(place > 0).then(rule)}
                        {{
                            let (index, muted) = (stream.index, stream.muted);
                            let slide = self.slides.iter().find(|(known, _)| *known == index).map(|(_, slide)| slide.clone()).unwrap_or_default();
                            let mute = rsx! {
                                <div
                                    base={press(speaker(stream.level, muted), muted)}
                                    id={("mute", index as usize)}
                                    onClick={cx.listener(move |page, _, _, cx| page.mute(index, !muted, cx))}
                                />
                            };
                            let label = if stream.playing.is_empty() { stream.name.clone() } else { format!("{}  ·  {}", stream.name, stream.playing) };
                            let says = if muted { "Muted".to_string() } else { format!("{}%", slide.showing(stream.level)) };
                            let set = move |value: i64, cx: &mut gpui::App| {
                                cx.background_spawn(async move { streams::set_level(index, value) }).detach();
                            };
                            rsx! {
                                <div class="flex flex-col flex-none">
                                    {level(mute, label, says)}
                                    <div class="flex-none pb-[10px]" when={(muted, |track| track.opacity(0.45))}>
                                        {slider(&slide, stream.level, (0, 150), set)}
                                    </div>
                                </div>
                            }
                        }}
                    </div>
                }}
            </div>
        }
    }
}

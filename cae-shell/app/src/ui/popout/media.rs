//! What is playing, and the three things to do about it.

use cae_core::media;
use gpui::{AppContext, Context, Div, IntoElement, Render, Stateful, Styled, Window, div, prelude::*, px};

use super::pieces::{column, detail, headline};
use crate::feeds::Feeds;
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::rsx;

pub struct Media {
    feeds: Feeds,
}

impl Media {
    pub fn new(feeds: &Feeds, cx: &mut Context<Self>) -> Media {
        cx.observe(&feeds.media, |_, _, cx| cx.notify()).detach();
        Media { feeds: feeds.clone() }
    }
}

/// One of the transport's buttons. The middle one is the bigger and the
/// brighter, because it is the one reached for.
fn control(action: &'static str, symbol: &'static str, primary: bool, enabled: bool) -> Stateful<Div> {
    let across = px(if primary { 34. } else { 30. });
    rsx! {
        <div
            id={action}
            class="flex flex-none items-center justify-center rounded-full"
            size={across}
            bg={theme::white(if primary { 0.16 } else { 0.07 })}
            text_color={if primary { theme::text() } else { theme::text_dim() }}
            when={(!enabled, |control| control.opacity(0.35))}
            when={(enabled, |control| {
                control
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::white(0.13)).text_color(theme::text()))
                    .on_click(move |_, _, cx| cx.background_spawn(async move { media::control(action) }).detach())
            })}
        >
            {glyph(symbol, px(18.))}
        </div>
    }
}

impl Render for Media {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(playing) = self.feeds.media.read(cx).value.clone() else {
            return rsx! { <div base={column()}>{headline("Nothing playing")}</div> };
        };
        let title = if playing.title.is_empty() { playing.identity.clone() } else { playing.title.clone() };

        rsx! {
            <div base={column()}>
                {headline(title)}
                {...(!playing.artist.is_empty()).then(|| detail(playing.artist.clone()))}
                <div class="flex flex-none items-center justify-center gap-[6px]">
                    {control("Previous", "skip_previous", false, playing.can_go_previous)}
                    {control("PlayPause", if playing.playing { "pause" } else { "play_arrow" }, true, true)}
                    {control("Next", "skip_next", false, playing.can_go_next)}
                </div>
                {detail(playing.identity.clone())}
            </div>
        }
    }
}

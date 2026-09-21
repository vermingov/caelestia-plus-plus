//! The pane: the glass at the foot of the screen, and how it comes and goes.

use std::time::Duration;

use gpui::{
    App, AppContext, Bounds, Context, IntoElement, Pixels, Render, Size, Styled, Task, WeakEntity, Window, canvas, div,
    point, prelude::*, px,
};

use super::{Utilities, cards};
use crate::ease::{Curve, Tween};
use crate::feeds::Feeds;
use crate::theme;
use crate::ui::{pointer, rsx};

const WIDTH: Pixels = px(430.);
/// As tall as the cards make it, and never taller than this: the surface has
/// to be made before they are laid out, and what is not the pane is nothing.
const TALLEST: Pixels = px(452.);
/// How far it stands off the edges of the screen, which is how far the bar
/// does.
const FLOAT: Pixels = px(12.);
/// How far below its resting place it starts.
const TRAVEL: f32 = 18.;
/// The float, the pane, and 48 above and to the left for the shadow it
/// casts. In numbers, because pixels cannot be added up in a constant.
pub const SURFACE: Size<Pixels> = Size { width: px(48. + 430. + 12.), height: px(48. + 452. + 12.) };

const ENTER: Duration = Duration::from_millis(260);
const LEAVE: Duration = Duration::from_millis(170);
/// Crossing from one thing in it to another takes the pointer off both for a
/// moment, and that must not count as leaving.
const GRACE: Duration = Duration::from_millis(260);
/// How long one the pointer has not reached yet waits for it: it was opened
/// by a key, and the hand may never come.
const UNCLAIMED: Duration = Duration::from_millis(2600);

pub struct Pane {
    keep_awake: gpui::Entity<cards::KeepAwake>,
    recorder: gpui::Entity<cards::Recorder>,
    toggles: gpui::Entity<cards::Toggles>,
    utilities: WeakEntity<Utilities>,
    /// Whether the pointer decides when it goes, which it does once it has
    /// been touched.
    follows_pointer: bool,
    closing: Option<Task<()>>,
    shown: Tween,
    leaving: bool,
}

impl Pane {
    /// `by_hover` when the pointer reached for the corner rather than a key
    /// being pressed: one that was reached for goes when the pointer does,
    /// and one opened by a key waits a moment for a hand that may never come.
    pub fn new(by_hover: bool, feeds: &Feeds, utilities: WeakEntity<Utilities>, window: &mut Window, cx: &mut Context<Self>) -> Pane {
        let mut shown = Tween::still(0.);
        shown.go(1., ENTER, Curve::Arrive);
        let mut pane = Pane {
            keep_awake: cx.new(cards::KeepAwake::new),
            recorder: cx.new(cards::Recorder::new),
            toggles: cx.new(|cx| cards::Toggles::new(feeds, cx)),
            utilities,
            follows_pointer: by_hover,
            closing: None,
            shown,
            leaving: false,
        };
        pane.close_after(UNCLAIMED, window, cx);
        pane
    }

    fn hovered(&mut self, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        if hovered {
            // Touched, so from here on it goes when the pointer does.
            (self.follows_pointer, self.closing) = (true, None);
        } else if self.follows_pointer {
            self.close_after(GRACE, window, cx);
        }
    }

    fn close_after(&mut self, wait: Duration, window: &mut Window, cx: &mut Context<Self>) {
        self.closing = Some(cx.spawn_in(window, async move |pane, cx| {
            cx.background_executor().timer(wait).await;
            let _ = pane.update_in(cx, |pane, window, cx| pane.leave(window, cx));
        }));
    }

    /// Plays the exit, and only then goes.
    pub fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.leaving, true) {
            return;
        }
        self.shown.go(0., LEAVE, Curve::In);
        cx.notify();
        let utilities = self.utilities.clone();
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(LEAVE).await;
            let _ = cx.update(|window, cx| {
                window.remove_window();
                let _ = utilities.update(cx, |utilities, _| utilities.gone());
            });
        })
        .detach();
    }
}

/// Tells the compositor which part of the surface is the pane: where the
/// cards ended up, and the gap between them and the two edges of the screen
/// the pane stands off. With the gap left out, a pointer sliding off the
/// pane towards the corner would be on nothing.
fn reach(pane: Bounds<Pixels>, window: &Window) {
    let region = Bounds::new(pane.origin - point(FLOAT, px(0.)), pane.size + Size::new(FLOAT, FLOAT));
    window.set_input_region(Some(&[region]));
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.shown.done() {
            window.request_animation_frame();
        }
        let shown = self.shown.value();

        rsx! {
            <div class="relative size-full" font_family={theme::FONT}>
                <div
                    id="utilities"
                    class="absolute flex flex-col justify-end"
                    right={px(0.)}
                    bottom={px(0.)}
                    w={FLOAT + WIDTH}
                    max_h={FLOAT + TALLEST}
                    onHover={cx.listener(|pane, hovered: &bool, window, cx| pane.hovered(*hovered, window, cx))}
                >
                    <div
                        class="relative flex flex-col flex-none gap-[8px] p-[12px]"
                        w={WIDTH}
                        max_h={TALLEST}
                        opacity={shown}
                        mt={px(TRAVEL * (1. - shown))}
                        mb={FLOAT}
                        rounded={px(20.)}
                        bg={theme::pane()}
                        shadow={theme::pane_shadows()}
                        text_color={theme::text()}
                    >
                        {self.keep_awake.clone()}
                        {self.recorder.clone()}
                        {self.toggles.clone()}
                        // Inside the glass, so that what the compositor is
                        // told is where the cards actually ended up.
                        <canvas
                            class="absolute size-full"
                            prepaint={|pane, window: &mut Window, _: &mut App| reach(pane, window)}
                            paint={|_, _, _, _| ()}
                        />
                    </div>
                </div>
                {pointer::see_out()}
            </div>
        }
    }
}

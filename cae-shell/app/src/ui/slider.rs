//! The one control the shell has that is dragged, so it is built rather than
//! borrowed: there is no slider to borrow.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, Bounds, DispatchPhase, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Styled, Window, canvas, div, prelude::*, px, relative,
};

use crate::theme;
use crate::ui::rsx;

/// The thumb, and so how far in from each end of the track its middle stops.
const THUMB: Pixels = px(12.);

/// What a slider has to remember from one frame to the next. Lives in the
/// panel that owns the slider, because the slider itself is rebuilt every
/// time it is drawn.
#[derive(Clone, Default)]
pub struct Slide {
    dragging: Rc<Cell<bool>>,
    /// Where the thumb has been put, until the thing it controls says the
    /// same. Setting a volume is a program started and an event waited for;
    /// a thumb that trails the pointer by that long feels broken.
    held: Rc<Cell<Option<i64>>>,
}

impl Slide {
    /// The value to say in words: where the thumb has been put while it is
    /// held, and what the machine reports otherwise. The number over a slider
    /// that lags the thumb under it reads as two controls disagreeing.
    pub fn showing(&self, reported: i64) -> i64 {
        self.held.get().unwrap_or(reported)
    }
}

/// A track from `lowest` to `highest` with the thumb at `level`. `set` is
/// called with each new whole value as the thumb is moved.
pub fn slider(
    slide: &Slide,
    level: i64,
    (lowest, highest): (i64, i64),
    set: impl Fn(i64, &mut App) + 'static,
) -> impl IntoElement {
    let shown = slide.held.get().unwrap_or(level).clamp(lowest, highest);
    let along = (shown - lowest) as f32 / (highest - lowest).max(1) as f32;
    let (slide, set) = (slide.clone(), Rc::new(set));

    let listen = move |travel: Bounds<Pixels>, window: &mut Window| {
        let value_at = move |x: Pixels| {
            let along = ((x - travel.left()) / travel.size.width).clamp(0., 1.);
            lowest + (along * (highest - lowest) as f32).round() as i64
        };
        let put = {
            let (slide, set) = (slide.clone(), set.clone());
            move |x: Pixels, window: &mut Window, cx: &mut App| {
                let value = value_at(x);
                if slide.held.replace(Some(value)) != Some(value) {
                    set(value, cx);
                    window.refresh();
                }
            }
        };

        window.on_mouse_event({
            let (slide, put) = (slide.clone(), put.clone());
            move |event: &MouseDownEvent, phase, window, cx| {
                // Anywhere on the row counts, not only on the four pixels of
                // track: the row is the target, the track is the picture.
                let on_row = travel.dilate(THUMB / 2.).contains(&event.position);
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left && on_row {
                    slide.dragging.set(true);
                    put(event.position.x, window, cx);
                }
            }
        });
        window.on_mouse_event({
            let slide = slide.clone();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && slide.dragging.get() && event.dragging() {
                    put(event.position.x, window, cx);
                }
            }
        });
        window.on_mouse_event({
            let slide = slide.clone();
            move |_: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !slide.dragging.replace(false) {
                    return;
                }
                // Let go of the thumb once the real value has had time to
                // arrive, and not before: released at once, it jumps back to
                // the old value and then forward again.
                let (slide, handle) = (slide.clone(), window.window_handle());
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(Duration::from_millis(400)).await;
                    if !slide.dragging.get() {
                        slide.held.set(None);
                        let _ = handle.update(cx, |_, window, _| window.refresh());
                    }
                })
                .detach();
            }
        });
    };

    rsx! {
        <div class="relative flex flex-none items-center h-[16px] cursor-pointer">
            <div class="flex-1 h-[4px] rounded-full" bg={theme::white(0.07)} />
            // The thumb's middle travels between the two points where its
            // edge meets an end of the track, so the box it moves in is the
            // track less half a thumb at each end.
            <div class="absolute h-full" left={THUMB / 2.} right={THUMB / 2.}>
                <canvas
                    class="absolute size-full"
                    prepaint={|_, _, _| ()}
                    paint={move |travel, _, window, _| listen(travel, window)}
                />
                <div
                    class="absolute rounded-full"
                    size={THUMB}
                    top={px(2.)}
                    left={relative(along)}
                    ml={-THUMB / 2.}
                    bg={theme::text()}
                    shadow={theme::thumb_shadow()}
                />
            </div>
        </div>
    }
}

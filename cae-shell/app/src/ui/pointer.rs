//! Seeing the pointer out.
//!
//! GPUI is told when the pointer leaves a window, and goes on believing it is
//! where it last was. It clears what was hovered, redraws because the window
//! has lost the pointer, finds the pointer (as far as it knows) still over
//! the same thing, and hovers it again. In an application that is a stuck
//! highlight now and then. On a bar it is every time the pointer is flicked
//! off an item and out of a surface fifty pixels tall, and a popout that is
//! told the pointer came back never closes.
//!
//! So as the pointer leaves, the window is sent one move to a place that is
//! in nothing. That lands before the redraw does, and by then there is
//! nothing left to get wrong.

use gpui::{DispatchPhase, IntoElement, MouseExitEvent, MouseMoveEvent, PlatformInput, Styled, canvas, point, px};

use crate::ui::rsx;

/// Goes in the root of every window the pointer can leave. Draws nothing and
/// takes no input: it is only a place to listen from.
pub fn see_out() -> impl IntoElement {
    rsx! {
        <canvas
            class="absolute size-full"
            prepaint={|_, _, _| ()}
            paint={|_, _, window, _| {
                window.on_mouse_event(|_: &MouseExitEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    // After this event rather than during it: one event at a
                    // time is what a window expects to be handling.
                    window.defer(cx, |window, cx| {
                        let nowhere = MouseMoveEvent {
                            position: point(px(-1.), px(-1.)),
                            pressed_button: None,
                            modifiers: window.modifiers(),
                        };
                        window.dispatch_event(PlatformInput::MouseMove(nowhere), cx);
                    });
                });
            }}
        />
    }
}

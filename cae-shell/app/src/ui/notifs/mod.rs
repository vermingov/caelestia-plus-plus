//! Notifications: the toasts, and the centre they collect in.
//!
//! Two kinds of surface on each screen. A sliver in the top corner that is
//! always there, because reaching into that corner is how the centre is
//! opened; and the column down the right-hand edge, which exists only while
//! there is a toast on it or the centre is open. Between notifications this
//! is two pixels of surface and a list in memory.

mod centre;
mod column;
mod markup;
mod parts;
mod toast;

use std::collections::HashMap;
use std::time::Duration;

use cae_core::hypr;
use gpui::{
    AnyWindowHandle, App, AppContext, AsyncApp, Bounds, Context, DisplayId, IntoElement, Render, Size, Styled,
    Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, layer_shell::*,
    point, prelude::*, px,
};

use crate::feeds::Feeds;
use crate::ui::rsx;
use crate::ui::screen::{self, STRETCH};
use column::Column;

/// What the compositor's rules match on: the old surface's name, so that the
/// rules written for it still apply.
const NAMESPACE: &str = "caelestia-notifs";

/// The corner that opens the centre, as the edge of the old sidebar did. A
/// sliver, so that it costs the windows beneath it nothing but their
/// outermost pixels.
const CORNER: Size<gpui::Pixels> = Size { width: px(2.), height: px(140.) };

struct Screen {
    name: String,
    corner: AnyWindowHandle,
    column: Option<WindowHandle<Column>>,
}

pub struct Surfaces {
    feeds: Feeds,
    screens: HashMap<DisplayId, Screen>,
}

/// Keeps the notification surfaces on every output for the life of the shell.
pub fn keep_on_every_output(cx: &mut App, feeds: &Feeds) {
    let surfaces = cx.new(|cx| {
        cx.observe(&feeds.notifs, |surfaces: &mut Surfaces, _, cx| surfaces.columns(cx)).detach();
        Surfaces { feeds: feeds.clone(), screens: HashMap::new() }
    });
    // Outputs come and go, and GPUI does not say when. Looked at on a slow
    // tick, quickly at first: at startup the displays arrive a few
    // milliseconds after the application does.
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut looks = 0_u32;
        loop {
            let found = surfaces.update(cx, |surfaces, cx| surfaces.corners(cx));
            looks += 1;
            let wait = if found == 0 && looks < 200 { 25 } else { 2000 };
            cx.background_executor().timer(Duration::from_millis(wait)).await;
        }
    })
    .detach();
}

/// A layer surface down the right-hand edge of `display`, over everything:
/// whether a notification may appear over a fullscreen window is a setting,
/// and a surface underneath one could never honour "yes". It never takes the
/// keyboard, which would take it from whatever the notification interrupted.
fn surface(display: DisplayId, size: Size<gpui::Pixels>, anchor: Anchor) -> WindowOptions {
    WindowOptions {
        titlebar: None,
        focus: false,
        display_id: Some(display),
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size))),
        app_id: Some(NAMESPACE.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: NAMESPACE.to_string(),
            layer: Layer::Overlay,
            anchor,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..crate::ui::surface::options()
    }
}

impl Surfaces {
    /// A corner on every output there is, and none on one that has gone.
    /// Says how many outputs there are.
    fn corners(&mut self, cx: &mut Context<Self>) -> usize {
        let outputs = screen::outputs(cx);
        self.screens.retain(|display, screen| {
            let stays = outputs.iter().any(|(_, wanted)| wanted == display);
            if !stays {
                let _ = screen.corner.update(cx, |_, window, _| window.remove_window());
                if let Some(column) = screen.column.take() {
                    let _ = column.update(cx, |_, window, _| window.remove_window());
                }
            }
            stays
        });

        for (name, display) in &outputs {
            if self.screens.contains_key(display) {
                continue;
            }
            let (feeds, output) = (self.feeds.clone(), name.clone());
            let options = surface(*display, CORNER, Anchor::TOP | Anchor::RIGHT);
            let sliver = move |_: &mut Window, cx: &mut App| {
                cx.new(|cx| {
                    // Whether the centre is already open here is part of what
                    // it draws, so it hears when that changes.
                    cx.observe(&feeds.notifs, |_, _, cx| cx.notify()).detach();
                    Corner { output, feeds }
                })
            };
            match cx.open_window(options, sliver) {
                Ok(corner) => {
                    let screen = Screen { name: name.clone(), corner: corner.into(), column: None };
                    self.screens.insert(*display, screen);
                }
                Err(error) => eprintln!("cae: cannot open the notification corner on {name}: {error}"),
            }
        }
        self.columns(cx);
        outputs.len()
    }

    /// A column wherever there is something to put in one. Each takes itself
    /// down when it has nothing left, so this only ever opens them.
    fn columns(&mut self, cx: &mut Context<Self>) {
        let feed = &self.feeds.notifs.read(cx).value;
        let popping = feed.list.iter().any(|notif| notif.popup);
        let centre = feed.centre.clone();

        let surfaces = cx.weak_entity();
        for (display, screen) in &mut self.screens {
            if screen.column.is_some() || !(popping || centre == screen.name) {
                continue;
            }
            let (feeds, output, display, surfaces) = (self.feeds.clone(), screen.name.clone(), *display, surfaces.clone());
            let options = surface(display, Size::new(column::SURFACE, STRETCH), Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM);
            match cx.open_window(options, move |_, cx| cx.new(|cx| Column::new(output, display, &feeds, surfaces, cx))) {
                Ok(column) => screen.column = Some(column),
                Err(error) => eprintln!("cae: cannot open the notification column on {}: {error}", screen.name),
            }
        }
    }

    /// A column has taken itself down.
    fn gone(&mut self, display: DisplayId) {
        if let Some(screen) = self.screens.get_mut(&display) {
            screen.column = None;
        }
    }
}

/// The sliver in the corner of one screen.
struct Corner {
    output: String,
    feeds: Feeds,
}

impl Render for Corner {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open_here = self.feeds.notifs.read(cx).value.centre == self.output;
        let (server, output) = (self.feeds.server.clone(), self.output.clone());
        rsx! {
            <div
                id="corner"
                class="size-full"
                // Any movement in it, rather than arriving in it. A window is
                // born believing the pointer is at its top left corner, which
                // for one this size is inside it; that uses up "the pointer
                // has arrived", and when the pointer really does it is no
                // longer news.
                onMouseMove={move |_: &gpui::MouseMoveEvent, _: &mut Window, cx: &mut App| {
                    if open_here {
                        return;
                    }
                    // Refused over a fullscreen window: the corner of a game
                    // is part of the game. Asked of the compositor away from
                    // the thread that draws.
                    let (server, output) = (server.clone(), output.clone());
                    cx.background_spawn(async move {
                        if !hypr::fullscreen_focused() {
                            server.set_centre(&output);
                        }
                    })
                    .detach();
                }}
            />
        }
    }
}

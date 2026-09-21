//! The bar: one layer surface per output, fifty pixels tall.
//!
//! Three things are stacked in it. The pill's face, which never changes; the
//! spectrum, which changes fifteen times a second while music plays; and the
//! strip of readouts on top, which changes about once a second. The last two
//! are views of their own and are embedded cached, so a frame of the spectrum
//! is a repaint of the spectrum and not a second layout of every pill.

mod outputs;
mod pieces;
mod readouts;
mod strip;
mod tray;
mod visualiser;
mod workspaces;

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, DisplayId, Entity, IntoElement, Pixels, Render, Size,
    StyleRefinement, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div,
    layer_shell::*, point, prelude::*, px,
};

pub use outputs::keep_on_every_output;

use crate::feeds::Feeds;
use crate::theme;
use crate::ui::rsx;
use crate::ui::{pointer, screen};

use strip::Strip;
use visualiser::Visualiser;

/// What the compositor's rules match on. The same name the old bar used, so
/// every rule written for it still applies.
const NAMESPACE: &str = "caelestia-bar";

pub struct Bar {
    strip: Entity<Strip>,
    visualiser: Entity<Visualiser>,
    feeds: Feeds,
}

impl Bar {
    fn new(output: String, feeds: &Feeds, cx: &mut Context<Self>) -> Bar {
        // The entry list decides whether the spectrum is drawn at all, and
        // that can change while the bar is up.
        cx.observe(&feeds.settings, |_, _, cx| cx.notify()).detach();
        Bar {
            strip: cx.new(|cx| Strip::new(output, feeds, cx)),
            visualiser: cx.new(|cx| Visualiser::new(feeds, cx)),
            feeds: feeds.clone(),
        }
    }
}

/// Where the pill sits in the surface: air above and to both sides.
fn pill_box() -> StyleRefinement {
    StyleRefinement::default().absolute().top(theme::FLOAT).left(theme::FLOAT).right(theme::FLOAT).h(theme::PILL)
}

impl Render for Bar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let spectrum = self.feeds.settings.read(cx).value.layout.entries.iter().any(|entry| entry == "visualiser");

        rsx! {
            <div class="relative size-full" font_family={theme::FONT}>
                <div
                    class="absolute rounded-full"
                    top={theme::FLOAT}
                    left={theme::FLOAT}
                    right={theme::FLOAT}
                    h={theme::PILL}
                    bg={theme::face()}
                    shadow={theme::face_shadows()}
                />
                {...spectrum.then(|| self.visualiser.clone().cached(pill_box()))}
                {self.strip.clone().cached(pill_box())}
                {pointer::see_out()}
            </div>
        }
    }
}

/// Opens a bar on one output.
fn open(cx: &mut App, display: DisplayId, output: String, feeds: &Feeds, preview: bool) -> Option<AnyWindowHandle> {
    let height = theme::PILL + theme::FLOAT;
    let options = WindowOptions {
        titlebar: None,
        display_id: Some(display),
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: Size::new(screen::STRETCH, height),
        })),
        app_id: Some(NAMESPACE.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(layer(preview, height)),
        ..Default::default()
    };

    let feeds = feeds.clone();
    let name = output.clone();
    match cx.open_window(options, move |_, cx| cx.new(|cx| Bar::new(name, &feeds, cx))) {
        Ok(window) => Some(window.into()),
        Err(error) => {
            eprintln!("cae: cannot open a bar on {output}: {error}");
            None
        }
    }
}

/// Across the top, reserving its own strip, never taking the keyboard.
///
/// A preview goes across the top as well, over everything and reserving
/// nothing. A surface that reserves nothing is placed clear of the ones that
/// do, so it lands directly under the bar that is in use: the two can be
/// compared a row apart, and the preview's popouts open downward as the real
/// ones will.
fn layer(preview: bool, height: Pixels) -> LayerShellOptions {
    if preview {
        return LayerShellOptions {
            namespace: format!("{NAMESPACE}-preview"),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        };
    }
    LayerShellOptions {
        namespace: NAMESPACE.to_string(),
        layer: Layer::Top,
        anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        exclusive_zone: Some(height),
        // A bar is not a place to type: taking the keyboard would swallow
        // every shortcut the moment the pointer crossed it.
        keyboard_interactivity: KeyboardInteractivity::None,
        ..Default::default()
    }
}

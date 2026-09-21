//! Choosing a piece of the screen, which is how a screenshot of less than
//! all of it is taken.
//!
//! One surface over each screen, and a rectangle dragged out on it. With
//! nothing dragged yet the rectangle is whatever window is under the
//! pointer, so the common case — a shot of one window — is a single click.
//! What is chosen goes to `caelestia screenshot`, which crops it and opens
//! the editor: the same command the keybind would have run for a fullscreen
//! shot, so a shot always ends up in the same place.

use std::path::PathBuf;
use std::rc::Rc;

use cae_core::hypr;
use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, DisplayId, FocusHandle, Focusable, Global, IntoElement,
    KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Render, Size, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, actions, div, img, layer_shell::*, point, prelude::*, px,
};

use crate::theme;
use crate::ui::screen::{self, STRETCH};
use crate::ui::{pointer, rsx};

actions!(picker, [Cancel]);

/// The name the compositor knows it by, which is also what a rule for it
/// would be written against.
const NAMESPACE: &str = "caelestia-area-picker";

/// A selection smaller than this in either direction is a click that meant
/// to take the window under it, not a region.
const A_SPECK: f32 = 8.;

/// What the shot is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    /// Hold the screen still while it is chosen from.
    pub freeze: bool,
    /// To the clipboard rather than to the editor.
    pub clip: bool,
}

impl Want {
    pub fn from(words: &mut dyn Iterator<Item = &str>) -> Want {
        let mut want = Want { freeze: false, clip: false };
        for word in words {
            match word {
                "freeze" => want.freeze = true,
                "clip" => want.clip = true,
                _ => {}
            }
        }
        want
    }
}

/// Once, at startup: what the keys do while it is up.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Cancel, Some("Picker"))]);
}

/// The surfaces that are up, so that whichever screen is chosen on can take
/// the rest away with it.
#[derive(Default)]
struct Up(Vec<AnyWindowHandle>);

impl Global for Up {}

/// Opens it on every screen. Already open: nothing, since it has the
/// keyboard and a second one could not be reached anyway.
pub fn ask(want: Want, cx: &mut App) {
    if !cx.default_global::<Up>().0.is_empty() {
        return;
    }
    let outputs = screen::outputs(cx);
    // Frozen before any surface is up, or the surfaces would be in the
    // picture. Each screen is grabbed on its own, because each is its own
    // picture.
    let frozen: Vec<Option<PathBuf>> = if want.freeze {
        outputs.iter().map(|(name, _)| freeze(name)).collect()
    } else {
        outputs.iter().map(|_| None).collect()
    };

    let showing = hypr::showing();
    let windows = hypr::windows();
    let monitors = hypr::monitors();
    let mut up = Vec::new();
    for ((name, display), frozen) in outputs.into_iter().zip(frozen) {
        // Where the compositor says this screen is among the others. One
        // that will not say is taken to be the only one there is, which is
        // right often enough and is better than no picker at all.
        let (x, y) = monitors.iter().find(|(monitor, ..)| *monitor == name).map_or((0, 0), |&(_, x, y, ..)| (x, y));
        // Only what is on the workspace this screen is showing, in the
        // screen's own coordinates.
        let on_screen = showing.iter().find(|(monitor, _)| *monitor == name).map(|(_, workspace)| *workspace);
        let here: Vec<Bounds<Pixels>> = windows
            .iter()
            .filter(|window| Some(window.workspace) == on_screen)
            .map(|window| {
                Bounds::new(
                    point(px((window.at.0 - x) as f32), px((window.at.1 - y) as f32)),
                    Size::new(px(window.size.0 as f32), px(window.size.1 as f32)),
                )
            })
            .collect();

        let at = Rc::new(Screen { x, y, windows: here, frozen, want });
        match cx.open_window(surface(display), move |window, cx| cx.new(|cx| Picker::new(at, window, cx))) {
            Ok(picker) => up.push(picker.into()),
            Err(error) => eprintln!("cae: cannot open the picker on {name}: {error}"),
        }
    }
    cx.set_global(Up(up));
}

/// Takes them all away.
/// Takes them all away. `window` is the one whose press or key this is: a
/// window cannot be asked to do anything while it is already doing
/// something, so that one takes itself away instead of being told to.
fn done(window: &mut Window, cx: &mut App) {
    let here = window.window_handle();
    for picker in std::mem::take(&mut cx.default_global::<Up>().0) {
        if picker != here {
            let _ = picker.update(cx, |_, window, _| window.remove_window());
        }
    }
    window.remove_window();
}

/// A still of one screen, kept where the pictures are.
fn freeze(name: &str) -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?
        .join("caelestia/picker");
    std::fs::create_dir_all(&cache).ok()?;
    let still = cache.join(format!("{name}.png"));
    let grabbed = std::process::Command::new("grim").args(["-l", "0", "-o", name]).arg(&still).status();
    grabbed.is_ok_and(|status| status.success()).then_some(still)
}

fn surface(display: DisplayId) -> WindowOptions {
    WindowOptions {
        titlebar: None,
        display_id: Some(display),
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), Size::new(STRETCH, STRETCH)))),
        app_id: Some(NAMESPACE.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: NAMESPACE.to_string(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            // Over the bar as well: a shot may be of the bar.
            exclusive_zone: Some(px(-1.)),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        }),
        ..crate::ui::surface::options()
    }
}

/// What one screen is, as far as the picker is concerned.
struct Screen {
    /// Where the screen is among the others, which is what turns a selection
    /// on it into one the compositor can crop.
    x: i32,
    y: i32,
    /// The windows on it, in the order they lie.
    windows: Vec<Bounds<Pixels>>,
    frozen: Option<PathBuf>,
    want: Want,
}

pub struct Picker {
    at: Rc<Screen>,
    /// Where the pointer went down, while it is still down.
    from: Option<gpui::Point<Pixels>>,
    /// What would be taken if it were let go of now.
    chosen: Option<Bounds<Pixels>>,
    focus: FocusHandle,
}

impl Picker {
    fn new(at: Rc<Screen>, window: &mut Window, cx: &mut Context<Self>) -> Picker {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Picker { at, from: None, chosen: None, focus }
    }

    /// The window under `at`, if any: the topmost, since the list is in the
    /// order they lie.
    fn window_under(&self, at: gpui::Point<Pixels>) -> Option<Bounds<Pixels>> {
        self.at.windows.iter().find(|window| window.contains(&at)).copied()
    }

    fn moved(&mut self, to: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let chosen = match self.from {
            Some(from) => Some(between(from, to)),
            None => self.window_under(to),
        };
        if chosen != self.chosen {
            self.chosen = chosen;
            cx.notify();
        }
    }

    /// Lets go: what is chosen is taken, unless it is too small to have been
    /// meant, in which case it is the window under the pointer.
    fn taken(&mut self, at: gpui::Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let dragged = self.from.take().map(|from| between(from, at));
        let big_enough = |region: &Bounds<Pixels>| region.size.width > px(A_SPECK) && region.size.height > px(A_SPECK);
        let Some(region) = dragged.filter(big_enough).or_else(|| self.window_under(at)) else {
            return done(window, cx);
        };
        let (x, y) = (self.at.x + f32::from(region.origin.x) as i32, self.at.y + f32::from(region.origin.y) as i32);
        let (width, height) = (f32::from(region.size.width) as i32, f32::from(region.size.height) as i32);
        let want = self.at.want;
        // Off the drawing thread, and after the surfaces have gone: a shot
        // taken while they are up is a shot of them.
        done(window, cx);
        cx.defer(move |cx| {
            cx.background_spawn(async move { take(x, y, width, height, want) }).detach();
        });
    }
}

/// The rectangle between two corners, whichever way round they were given.
///
/// `Bounds::from_corners` takes the second from the first and no more, so a
/// drag up or to the left comes out with a negative width: too small to be
/// "big enough", drawn inside out, and a region `grim` would refuse. A
/// rectangle has no direction, and neither should the drag that makes one.
fn between(one: gpui::Point<Pixels>, other: gpui::Point<Pixels>) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(one.x.min(other.x), one.y.min(other.y)),
        point(one.x.max(other.x), one.y.max(other.y)),
    )
}

/// Hands the region to the CLI, which crops it and does what the keybind
/// would have done with a whole screen.
fn take(x: i32, y: i32, width: i32, height: i32, want: Want) {
    let region = format!("{x},{y} {width}x{height}");
    let mut asking = std::process::Command::new("caelestia");
    asking.args(["screenshot", "-r", &region]);
    if want.clip {
        asking.arg("--clipboard");
    }
    let _ = asking.status();
}

impl Focusable for Picker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Picker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.chosen;

        rsx! {
            <div
                id="picker"
                class="relative size-full cursor-crosshair"
                key_context="Picker"
                track_focus={&self.focus}
                on_action={|_: &Cancel, window: &mut Window, cx: &mut App| done(window, cx)}
                onMouseDown={(MouseButton::Left, cx.listener(|picker: &mut Picker, event: &MouseDownEvent, _, cx| {
                    picker.from = Some(event.position);
                    picker.moved(event.position, cx);
                }))}
                onMouseMove={cx.listener(|picker: &mut Picker, event: &MouseMoveEvent, _, cx| picker.moved(event.position, cx))}
                onMouseUp={(MouseButton::Left, cx.listener(|picker: &mut Picker, event: &MouseUpEvent, window, cx| picker.taken(event.position, window, cx)))}
                // The other button is "never mind", as Escape is.
                onMouseUp={(MouseButton::Right, |_, window: &mut Window, cx: &mut App| done(window, cx))}
            >
                {...self.at.frozen.clone().map(|still| rsx! {
                    <img class="absolute size-full" src={still} object_fit={ObjectFit::Fill} />
                })}
                {...shade(chosen)}
                {...chosen.map(|region| rsx! {
                    <div
                        class="absolute"
                        left={region.origin.x}
                        top={region.origin.y}
                        w={region.size.width}
                        h={region.size.height}
                        border={px(2.)}
                        border_color={theme::accent()}
                    />
                })}
                {pointer::see_out()}
            </div>
        }
    }
}

/// The dark over everything that is not chosen: four pieces around it,
/// because a rectangle cannot have a hole in it.
fn shade(chosen: Option<Bounds<Pixels>>) -> Vec<gpui::Div> {
    const DARK: f32 = 0.35;
    let Some(region) = chosen else {
        return vec![rsx! { <div class="absolute size-full" bg={theme::black(DARK)} /> }];
    };
    let (left, top) = (region.origin.x, region.origin.y);
    let (right, bottom) = (left + region.size.width, top + region.size.height);
    let none = px(0.);
    vec![
        rsx! { <div class="absolute" left={none} right={none} top={none} h={top} bg={theme::black(DARK)} /> },
        rsx! { <div class="absolute" left={none} right={none} bottom={none} top={bottom} bg={theme::black(DARK)} /> },
        rsx! { <div class="absolute" left={none} top={top} w={left} h={region.size.height} bg={theme::black(DARK)} /> },
        rsx! { <div class="absolute" right={none} top={top} left={right} h={region.size.height} bg={theme::black(DARK)} /> },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_makes_the_same_rectangle_whichever_way_it_is_pulled() {
        let corner = |x: f32, y: f32| point(px(x), px(y));
        let down_right = between(corner(100., 100.), corner(300., 250.));
        for from_to in [
            (corner(300., 250.), corner(100., 100.)),
            (corner(300., 100.), corner(100., 250.)),
            (corner(100., 250.), corner(300., 100.)),
        ] {
            assert_eq!(between(from_to.0, from_to.1), down_right);
        }
        assert_eq!(down_right.origin, corner(100., 100.));
        assert_eq!(down_right.size.width, px(200.));
        assert_eq!(down_right.size.height, px(150.));
    }

    #[test]
    fn a_rectangle_pulled_backwards_is_still_big_enough_to_mean_it() {
        let corner = |x: f32, y: f32| point(px(x), px(y));
        let pulled_back = between(corner(400., 400.), corner(100., 100.));
        assert!(pulled_back.size.width > px(A_SPECK) && pulled_back.size.height > px(A_SPECK));
    }
}

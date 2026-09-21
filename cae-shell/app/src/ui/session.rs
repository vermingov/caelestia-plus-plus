//! The session menu: logging out, and the ways of putting the machine down.
//!
//! The one thing the shell draws that ends everything else, so it is the one
//! thing that takes the whole screen and the keyboard with it: a press
//! anywhere else, or Escape, is "never mind", and nothing under it can be
//! pressed by mistake on the way to it.

use std::path::PathBuf;
use std::time::Duration;

use cae_core::{config, services, session};
use gpui::{
    App, AppContext, Bounds, Context, FocusHandle, Focusable, Global, IntoElement, KeyBinding, MouseButton, ObjectFit, Pixels, Render, Size,
    Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, actions, div, img, layer_shell::*,
    point, prelude::*, px,
};

use crate::ease::{Curve, Tween};
use crate::ours;
use crate::theme;
use crate::ui::glyph::glyph;
use crate::ui::screen::{self, STRETCH};
use crate::ui::{Ask, pointer, rsx};

actions!(session, [Dismiss, Next, Previous, Choose]);

/// What the keys do while the menu is up. Once, at startup.
pub fn bind_keys(cx: &mut App) {
    const CONTEXT: Option<&str> = Some("Session");
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, CONTEXT),
        KeyBinding::new("down", Next, CONTEXT),
        KeyBinding::new("tab", Next, CONTEXT),
        KeyBinding::new("ctrl-j", Next, CONTEXT),
        KeyBinding::new("ctrl-n", Next, CONTEXT),
        KeyBinding::new("up", Previous, CONTEXT),
        KeyBinding::new("shift-tab", Previous, CONTEXT),
        KeyBinding::new("ctrl-k", Previous, CONTEXT),
        KeyBinding::new("ctrl-p", Previous, CONTEXT),
        KeyBinding::new("enter", Choose, CONTEXT),
    ]);
}

const PIECE: &str = "session";
const NAMESPACE: &str = "caelestia-panel";

const WIDTH: Pixels = px(228.);
const ROW: f32 = 52.;
const TRAVEL: Duration = Duration::from_millis(190);
const ENTER: Duration = Duration::from_millis(220);

/// One of the four, with the command the config gives it.
struct Way {
    label: &'static str,
    glyph: String,
    command: Vec<String>,
}

/// The four, as the config has them, with upstream's defaults for whatever it
/// does not say.
fn ways() -> (Vec<Way>, Option<PathBuf>) {
    let shell = config::read(config::File::Shell);
    let said = |path: &str| config::lookup(&shell, path).cloned();
    let way = |label: &'static str, key: &str, glyph: &str, word: &str| {
        let command = said(&format!("session.commands.{key}"))
            .and_then(|command| serde_json::from_value::<Vec<String>>(command).ok())
            .filter(|command| !command.is_empty())
            .unwrap_or_else(|| vec![word.to_string()]);
        let glyph = said(&format!("session.icons.{key}")).and_then(|glyph| glyph.as_str().map(str::to_string)).unwrap_or_else(|| glyph.to_string());
        Way { label, glyph, command }
    };
    let ways = vec![
        way("Log out", "logout", "logout", "logout"),
        way("Shut down", "shutdown", "power_settings_new", "poweroff"),
        way("Hibernate", "hibernate", "downloading", "hibernate"),
        way("Restart", "reboot", "cached", "reboot"),
    ];

    // The picture that keeps the menu company: the config's, or the one the
    // shell has always shipped. `root:` is the shell's own directory.
    let picture = said("paths.sessionGif").and_then(|path| path.as_str().map(str::to_string)).unwrap_or_else(|| "root:/assets/kurukuru.gif".to_string());
    let picture = match picture.strip_prefix("root:/") {
        Some(inside) => cae_core::about::checkout().join(inside),
        None => PathBuf::from(picture),
    };
    (ways, Some(picture).filter(|picture| picture.is_file()))
}

fn enabled() -> bool {
    let shell = config::read(config::File::Shell);
    config::lookup(&shell, "session.enabled").and_then(serde_json::Value::as_bool).unwrap_or(true)
}

#[derive(Default)]
struct Open(Option<WindowHandle<Menu>>);

impl Global for Open {}

/// Opens the menu, or shuts it if it is open. While the menu is still
/// Quickshell's to draw, Quickshell is asked instead.
pub fn ask(ask: Ask, cx: &mut App) {
    ours::when_known(PIECE, cx, move |ours, cx| {
        if ours {
            return ask_ours(ask, cx);
        }
        cx.background_spawn(async { drop(services::ipc("drawers", "toggle", &["session"])) }).detach();
    });
}

fn ask_ours(ask: Ask, cx: &mut App) {
    match (ask, cx.default_global::<Open>().0.take()) {
        // Asked to show, and it already is.
        (Ask::Show, Some(menu)) => cx.default_global::<Open>().0 = Some(menu),
        (Ask::Hide | Ask::Toggle, Some(menu)) => drop(menu.update(cx, |_, window, _| window.remove_window())),
        (Ask::Hide, None) => {}
        (Ask::Show | Ask::Toggle, None) => open(cx),
    }
}

/// Takes the menu away, however it was told to go: by Escape, by a press
/// outside it, or by one of its own buttons. What is kept of it goes with
/// it, or the next ask would find a window that is not there.
fn away(window: &mut Window, cx: &mut App) {
    cx.set_global(Open(None));
    window.remove_window();
}

fn open(cx: &mut App) {
    if !enabled() {
        return;
    }

    let options = WindowOptions {
        titlebar: None,
        display_id: screen::focused_display(cx),
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), Size::new(STRETCH, STRETCH)))),
        app_id: Some(NAMESPACE.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: NAMESPACE.to_string(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            // Over the bar too: the menu is centred on the screen, not on
            // what the bar leaves of it.
            exclusive_zone: Some(px(-1.)),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        }),
        ..crate::ui::surface::options()
    };
    match cx.open_window(options, |window, cx| cx.new(|cx| Menu::new(window, cx))) {
        Ok(menu) => cx.set_global(Open(Some(menu))),
        Err(error) => eprintln!("cae: cannot open the session menu: {error}"),
    }
}

pub struct Menu {
    ways: Vec<Way>,
    picture: Option<PathBuf>,
    chosen: usize,
    marker: Tween,
    shown: Tween,
    focus: FocusHandle,
}

impl Menu {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Menu {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let (ways, picture) = ways();
        let mut shown = Tween::still(0.);
        shown.go(1., ENTER, Curve::Arrive);
        Menu { ways, picture, chosen: 0, marker: Tween::still(0.), shown, focus }
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let count = self.ways.len() as isize;
        self.choose((self.chosen as isize + by).rem_euclid(count) as usize, cx);
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if index != self.chosen {
            self.chosen = index;
            self.marker.go(index as f32, TRAVEL, Curve::Settle);
            cx.notify();
        }
    }

    fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let command = self.ways[self.chosen].command.clone();
        cx.background_spawn(async move { session::run(&command) }).detach();
        away(window, cx);
    }
}

impl Focusable for Menu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Menu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !(self.marker.done() && self.shown.done()) {
            window.request_animation_frame();
        }
        let (marker, shown) = (self.marker.value(), self.shown.value());

        let pane = rsx! {
            <div
                id="ways"
                class="relative flex flex-col flex-none p-[8px]"
                // A press on the menu is not a press outside it.
                occlude
                w={WIDTH}
                opacity={shown}
                mr={px(18. - 14. * (1. - shown))}
                rounded={px(15.)}
                bg={theme::pane()}
                shadow={theme::pane_shadows()}
                text_color={theme::text()}
            >
                <div
                    class="absolute"
                    top={px(8. + ROW * marker)}
                    left={px(8.)}
                    right={px(8.)}
                    h={px(ROW)}
                    rounded={px(10.)}
                    bg={theme::highlight()}
                    shadow={theme::highlight_shadows()}
                />
                {for (index, way) in self.ways.iter().enumerate() {
                    <div
                        id={("way", index)}
                        class="relative flex flex-none items-center gap-[14px] px-[16px] cursor-pointer"
                        h={px(ROW)}
                        text_size={px(14.)}
                        text_color={if index == self.chosen { theme::text() } else { theme::text_dim() }}
                        // Only a pointer that moved chooses: one that the
                        // menu opened under has not chosen anything.
                        onMouseMove={cx.listener(move |menu, _, _, cx| menu.choose(index, cx))}
                        onClick={cx.listener(move |menu, _, window, cx| {
                            menu.choose(index, cx);
                            menu.run(window, cx);
                        })}
                    >
                        {glyph(way.glyph.clone(), px(22.))}
                        {way.label}
                    </div>
                }}
                {...self.picture.clone().map(|picture| rsx! {
                    <div class="flex flex-none items-center justify-center pt-[6px] pb-[2px]">
                        <img src={picture} class="flex-none w-[150px] h-[110px]" object_fit={ObjectFit::Contain} />
                    </div>
                })}
            </div>
        };

        rsx! {
            <div
                id="stage"
                class="relative size-full flex items-center justify-end"
                key_context="Session"
                track_focus={&self.focus}
                font_family={theme::FONT}
                onMouseDown={(MouseButton::Left, |_, window: &mut Window, cx: &mut App| away(window, cx))}
                on_action={|_: &Dismiss, window: &mut Window, cx: &mut App| away(window, cx)}
                on_action={cx.listener(|menu, _: &Next, _, cx| menu.step(1, cx))}
                on_action={cx.listener(|menu, _: &Previous, _, cx| menu.step(-1, cx))}
                on_action={cx.listener(|menu, _: &Choose, window, cx| menu.run(window, cx))}
            >
                {pane}
                {pointer::see_out()}
            </div>
        }
    }
}

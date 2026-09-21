//! The machine's modes, behind the wrench in the bar.
//!
//! Four switches, each of which changes how the machine behaves rather than
//! how it looks. Two of them have a privileged half that installs itself the
//! first time they are turned on, which is the one place in the shell that
//! ever asks for a password.

use cae_core::features::{self, Mode};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, Global, IntoElement, KeyBinding,
    MouseButton, Render, Size, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, actions, div, layer_shell::*, point, prelude::*, px,
};

use crate::ours;
use crate::theme;
use crate::ui::controls::switch;
use crate::ui::glyph::glyph;
use crate::ui::screen::{self, STRETCH};
use crate::ui::{pointer, rsx};

actions!(features, [Close]);

/// What Quickshell is asked about before this is drawn.
const PIECE: &str = "features";

const NAMESPACE: &str = "caelestia-panel";

/// How often the switches are read again while the menu is open: turning one
/// on sets a file and sometimes runs an installer, and the answer arrives
/// when it arrives.
const LOOK_AGAIN: std::time::Duration = std::time::Duration::from_millis(900);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Close, Some("Features"))]);
}

struct Menus {
    open: Option<WindowHandle<Menu>>,
}

struct Shared(Entity<Menus>);

impl Global for Shared {}

pub fn keep(cx: &mut App) {
    let menus = cx.new(|_| Menus { open: None });
    cx.set_global(Shared(menus));
}

/// Shows the menu, or shuts it if it is up. While it is still Quickshell's,
/// Quickshell is asked instead.
pub fn toggle(cx: &mut App) {
    let Some(shared) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return };
    ours::when_known(PIECE, cx, move |ours, cx| {
        if !ours {
            return cx.background_spawn(async { drop(cae_core::services::ipc("features", "toggleMenu", &[])) }).detach();
        }
        shared.update(cx, |menus, cx| menus.toggle(cx));
    });
}

impl Menus {
    fn toggle(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.open.take() {
            let _ = open.update(cx, |menu, window, cx| menu.leave(window, cx));
            return;
        }
        let menus = cx.weak_entity();
        let display = screen::focused_display(cx).or_else(|| screen::outputs(cx).first().map(|(_, display)| *display));
        let options = WindowOptions {
            titlebar: None,
            display_id: display,
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), Size::new(STRETCH, STRETCH)))),
            app_id: Some(NAMESPACE.to_string()),
            window_background: WindowBackgroundAppearance::Transparent,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: NAMESPACE.to_string(),
                layer: Layer::Overlay,
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                exclusive_zone: Some(px(-1.)),
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                ..Default::default()
            }),
            ..crate::ui::surface::options()
        };
        let opened = cx.open_window(options, move |window, cx| cx.new(|cx| Menu::new(menus, window, cx)));
        self.open = opened.map_err(|error| eprintln!("cae: cannot open the features menu: {error}")).ok();
    }

    fn gone(&mut self) {
        self.open = None;
    }
}

pub struct Menu {
    menus: gpui::WeakEntity<Menus>,
    modes: Vec<Mode>,
    /// The one whose installer is being waited on, so the row can say so.
    installing: Option<&'static str>,
    focus: FocusHandle,
    _looking: gpui::Task<()>,
}

impl Menu {
    fn new(menus: gpui::WeakEntity<Menus>, window: &mut Window, cx: &mut Context<Self>) -> Menu {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let looking = cx.spawn(async move |menu, cx| {
            loop {
                let modes = cx.background_spawn(async { features::modes() }).await;
                let landed = menu.update(cx, |menu: &mut Menu, cx| {
                    if menu.modes != modes {
                        // Whatever the installer was asked for has landed.
                        if let Some(waiting) = menu.installing
                            && modes.iter().any(|mode| mode.id == waiting && mode.ready)
                        {
                            menu.installing = None;
                        }
                        menu.modes = modes;
                        cx.notify();
                    }
                });
                if landed.is_err() {
                    return;
                }
                cx.background_executor().timer(LOOK_AGAIN).await;
            }
        });
        Menu { menus, modes: features::modes(), installing: None, focus, _looking: looking }
    }

    fn set(&mut self, mode: &Mode, cx: &mut Context<Self>) {
        let (id, on) = (mode.id, !mode.enabled);
        if on && !mode.ready {
            self.installing = Some(id);
        }
        // Shown at once: the file is written in a moment, and an installer
        // may take as long as it takes to type a password.
        if let Some(shown) = self.modes.iter_mut().find(|known| known.id == id) {
            shown.enabled = on;
        }
        cx.notify();
        cx.background_spawn(async move { features::set(id, on) }).detach();
    }

    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let menus = self.menus.clone();
        window.remove_window();
        cx.defer(move |cx| {
            let _ = menus.update(cx, |menus, _| menus.gone());
        });
    }
}

impl Focusable for Menu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.modes.iter().filter(|mode| mode.enabled).count();
        let rows: Vec<AnyElement> = self
            .modes
            .clone()
            .into_iter()
            .enumerate()
            .map(|(at, mode)| {
                let installing = self.installing == Some(mode.id);
                let said = if installing { "Setting up the privileged half — enter your password" } else { mode.said };
                rsx! {
                    <div
                        id={("mode", at)}
                        class="flex flex-none items-center gap-[14px] p-[13px] cursor-pointer"
                        rounded={px(13.)}
                        bg={theme::white(0.045)}
                        hover={|style| style.bg(theme::white(0.07))}
                        onClick={cx.listener({
                            let mode = mode.clone();
                            move |menu: &mut Menu, _, _, cx| menu.set(&mode, cx)
                        })}
                    >
                        <div
                            class="flex flex-none items-center justify-center size-[38px] rounded-full"
                            bg={if mode.enabled { theme::accent() } else { theme::white(0.08) }}
                            text_color={if mode.enabled { theme::on_accent() } else { theme::text_dim() }}
                        >
                            {glyph(mode.glyph, px(20.))}
                        </div>
                        <div class="flex flex-col flex-1 min-w-[0px] gap-[2px]">
                            <div class="flex-none" text_size={px(13.5)}>{mode.name}</div>
                            <div class="flex-none" text_size={px(11.5)} text_color={theme::text_dim()}>{said}</div>
                        </div>
                        {switch(mode.enabled)}
                    </div>
                }
                .into_any_element()
            })
            .collect();

        rsx! {
            <div
                id="features"
                class="relative size-full flex items-start justify-center"
                key_context="Features"
                track_focus={&self.focus}
                font_family={theme::FONT}
                bg={theme::black(0.35)}
                on_action={cx.listener(|menu: &mut Menu, _: &Close, window, cx| menu.leave(window, cx))}
                onMouseDown={(MouseButton::Left, cx.listener(|menu: &mut Menu, _, window, cx| menu.leave(window, cx)))}
            >
                <div
                    class="flex flex-col flex-none gap-[8px] p-[16px] w-[460px]"
                    mt={px(70.)}
                    rounded={px(20.)}
                    bg={theme::pane()}
                    shadow={theme::pane_shadows()}
                    text_color={theme::text()}
                    onMouseDown={(MouseButton::Left, |_, _: &mut Window, cx: &mut App| cx.stop_propagation())}
                >
                    <div class="flex flex-none items-baseline gap-[8px] px-[4px] pb-[4px]">
                        <div class="flex-1" text_size={px(15.)}>"Modes"</div>
                        <div text_size={px(11.5)} text_color={theme::text_faint()}>
                            {format!("{on} on")}
                        </div>
                    </div>
                    {...rows}
                </div>
                {pointer::see_out()}
            </div>
        }
    }
}

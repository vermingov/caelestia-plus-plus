//! The password a network asks for.
//!
//! Not in the popout the network was chosen from, which is on the screen
//! because the pointer is and has never been given the keyboard. This is a
//! window of its own that has it, in the place the popout was, and like the
//! launcher it is the whole screen with one pane on it: that is what lets a
//! click anywhere else be heard as "never mind".

use cae_core::services;
use gpui::{
    App, AppContext, Bounds, Context, DisplayId, Entity, Focusable, IntoElement, KeyBinding, MouseButton, Pixels,
    Point, Render, Size, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, actions,
    div, layer_shell::*, point, prelude::*, px,
};

use super::pieces::{chip, column, detail, headline};
use super::WIDTH;
use crate::theme;
use crate::ui::field::Field;
use crate::ui::screen::STRETCH;
use crate::ui::{pointer, rsx};

actions!(join, [Submit, Cancel]);

/// What the keys do while a password is being asked for. Once, at startup.
pub fn bind_keys(cx: &mut App) {
    const CONTEXT: Option<&str> = Some("Join");
    cx.bind_keys([KeyBinding::new("enter", Submit, CONTEXT), KeyBinding::new("escape", Cancel, CONTEXT)]);
}

pub struct Join {
    ssid: String,
    /// Where on the screen the pane goes: where the popout it replaces was.
    at: Point<Pixels>,
    password: Entity<Field>,
    joining: bool,
    failure: String,
}

/// Asks for `ssid`'s password with a pane whose top left corner is `at`, on
/// the display the popout was on.
pub fn ask(ssid: String, at: Point<Pixels>, display: Option<DisplayId>, cx: &mut App) {
    let options = WindowOptions {
        titlebar: None,
        display_id: display,
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), Size::new(STRETCH, STRETCH)))),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            // The name the compositor already has a rule for: blurred behind, and
            // faded in rather than slid.
            namespace: "caelestia-panel".to_string(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            // Over the bar as well as under it: the pane is placed in the
            // bar's own coordinates, which start at the top of the screen.
            exclusive_zone: Some(px(-1.)),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        }),
        ..Default::default()
    };
    let opened = cx.open_window(options, move |window, cx| {
        cx.new(|cx| {
            let password = cx.new(|cx| Field::new(format!("Password for {ssid}"), cx).secret());
            window.focus(&password.focus_handle(cx), cx);
            Join { ssid, at, password, joining: false, failure: String::new() }
        })
    });
    if let Err(error) = opened {
        eprintln!("cae: cannot ask for a password: {error}");
    }
}

impl Join {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (ssid, password) = (self.ssid.clone(), self.password.read(cx).text().to_string());
        if self.joining || password.is_empty() {
            return;
        }
        self.joining = true;
        self.failure.clear();
        cx.notify();

        cx.spawn_in(window, async move |join, cx| {
            let joined = cx.background_spawn(async move { services::join(&ssid, &password) }).await;
            let _ = join.update_in(cx, |join, window, cx| match joined {
                Ok(()) => window.remove_window(),
                // Stays up with what went wrong, which for a password is
                // nearly always the password.
                Err(failure) => {
                    (join.joining, join.failure) = (false, failure);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl Render for Join {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        rsx! {
            <div
                id="stage"
                class="relative size-full"
                key_context="Join"
                font_family={theme::FONT}
                onMouseDown={(MouseButton::Left, |_, window: &mut Window, _: &mut App| window.remove_window())}
                on_action={cx.listener(|join, _: &Submit, window, cx| join.submit(window, cx))}
                on_action={|_: &Cancel, window: &mut Window, _: &mut App| window.remove_window()}
            >
                <div
                    id="pane"
                    class="absolute flex flex-col py-[15px] px-[17px]"
                    occlude
                    left={self.at.x}
                    top={self.at.y}
                    w={WIDTH}
                    rounded={theme::RADIUS}
                    bg={theme::panel()}
                    shadow={theme::panel_shadows()}
                    text_color={theme::text()}
                >
                    <div base={column()}>
                        {headline(self.ssid.clone())}
                        <div class="flex items-center gap-[6px]">
                            <div
                                class="flex flex-1 items-center min-w-[0px] h-[24px] px-[9px] rounded-full"
                                bg={theme::white(0.07)}
                                text_size={px(12.)}
                            >
                                {self.password.clone()}
                            </div>
                            <div
                                base={chip(if self.joining { "…" } else { "Join" }, true)}
                                id="join"
                                when={(self.joining, |button| button.opacity(0.5))}
                                onClick={cx.listener(|join, _, window, cx| join.submit(window, cx))}
                            />
                        </div>
                        {...(!self.failure.is_empty()).then(|| detail(self.failure.clone()).text_color(theme::alert()))}
                        {detail("Enter to join, Escape to leave it")}
                    </div>
                </div>
                {pointer::see_out()}
            </div>
        }
    }
}

//! What the machine is missing, said once, unasked.
//!
//! The scan looks over the machine every time the settings' System scan page
//! is opened. Most of what it finds is worth knowing and nothing more. Two
//! kinds of finding are not: a package the shell needs and has not got, and
//! a privileged half left behind by an update — neither mends itself, and
//! both leave a feature quietly broken until somebody is told.
//!
//! So the shell looks once, a while after it starts, and puts what it found
//! in front of the person. Saying "later" to a thing means later for that
//! thing: the same missing package stays quiet, a newer version of the same
//! half asks again.

use std::collections::HashMap;
use std::time::Duration;

use cae_core::checkup::{self, Dismissed, Fix};
use cae_core::{config, tell};
use gpui::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, Global, IntoElement, KeyBinding, Render,
    SharedString, Size, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, actions, div, layer_shell::*, point, prelude::*, px,
};

use crate::ours;
use crate::theme;
use crate::ui::controls::chip;
use crate::ui::glyph::glyph;
use crate::ui::screen::{self, STRETCH};
use crate::ui::{rsx, settings};

actions!(setup, [Later]);

const NAMESPACE: &str = "caelestia-panel";

/// The same piece the settings page answers for: one shell looks over the
/// machine, and it is the one that says so.
const PIECE: &str = "scan";

/// Long enough after startup that the desktop is up and nothing is competing
/// for the disk. The scan itself takes the better part of a minute.
const FIRST_LOOK: Duration = Duration::from_secs(20);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Later, Some("Setup"))]);
}

/// What one look found that is worth interrupting somebody over.
#[derive(Clone, Debug, Default)]
struct Missing {
    packages: Vec<String>,
    /// The privileged halves, by the directory they install from.
    halves: Vec<String>,
    /// What version each half is at, kept from the look that found them:
    /// waving one away is waving away *that* version, and looking again to
    /// find out which takes the better part of a minute.
    versions: HashMap<String, checkup::Version>,
    /// The one press that mends all of it.
    fix: Option<Fix>,
}

impl Missing {
    fn anything(&self) -> bool {
        self.fix.is_some()
    }

    /// Looks over the machine and keeps only what has not already been
    /// waved away. Blocking.
    fn found() -> Missing {
        let report = checkup::scan();
        let waved = Dismissed::read();
        let packages = waved.fresh_packages(&report.missing_packages());
        let halves = waved.fresh_halves(&report.outdated_halves(), &report.halves);
        let fix = checkup::everything(&packages, &halves, &cae_core::about::checkout());
        Missing { packages, halves, versions: report.halves, fix }
    }

    /// What it amounts to, in a line.
    fn said(&self) -> SharedString {
        let count = |many: usize, one: &str| match many {
            1 => format!("one {one}"),
            many => format!("{many} {one}s"),
        };
        match (self.packages.len(), self.halves.len()) {
            (0, halves) => format!("{} out of date", count(halves, "privileged component")).into(),
            (packages, 0) => format!("{} missing", count(packages, "package")).into(),
            (packages, halves) => format!("{} and {}", count(packages, "package"), count(halves, "component")).into(),
        }
    }
}

struct Looking {
    open: Option<WindowHandle<Prompt>>,
}

struct Shared(Entity<Looking>);

impl Global for Shared {}

/// A settings file that will not parse reads as nothing, and everything in
/// it goes silently back to its default. That is worth one line, at once,
/// rather than at the end of a scan a minute later.
fn complain_about_the_settings() {
    for file in [config::File::Shell, config::File::Prefs] {
        if let Some(why) = config::complaint(file) {
            tell::tell("Settings not read", &why, "settings_alert", tell::How::Warned);
        }
    }
}

/// Looks once, a while after the shell is up.
pub fn keep(cx: &mut App) {
    let looking = cx.new(|_| Looking { open: None });
    cx.set_global(Shared(looking));
    cx.background_spawn(async { complain_about_the_settings() }).detach();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(FIRST_LOOK).await;
        let _ = cx.update(|cx| {
            ours::when_known(PIECE, cx, |ours, cx| {
                if ours {
                    look(cx);
                }
            })
        });
    })
    .detach();
}

/// Looks now. Also what `cae-shell scan` reaches.
pub fn look(cx: &mut App) {
    cx.spawn(async move |cx| {
        let missing = cx.background_spawn(async { Missing::found() }).await;
        if !missing.anything() {
            return;
        }
        let _ = cx.update(|cx| show(missing, cx));
    })
    .detach();
}

fn show(missing: Missing, cx: &mut App) {
    let Some(shared) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return };
    shared.update(cx, |looking, cx| {
        if let Some(open) = looking.open.take() {
            let _ = open.update(cx, |_, window, _| window.remove_window());
        }
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
            ..Default::default()
        };
        let opened = cx.open_window(options, move |window, cx| cx.new(|cx| Prompt::new(missing, window, cx)));
        looking.open = opened.map_err(|error| eprintln!("cae: cannot say what the machine is missing: {error}")).ok();
    });
}

pub struct Prompt {
    missing: Missing,
    focus: FocusHandle,
}

impl Prompt {
    fn new(missing: Missing, window: &mut Window, cx: &mut Context<Self>) -> Prompt {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Prompt { missing, focus }
    }

    /// Not now. What was found is written down, so the same thing stays
    /// quiet until it changes.
    fn later(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let missing = self.missing.clone();
        cx.background_spawn(async move {
            Dismissed::read().wave_away(&missing.packages, &missing.halves, &missing.versions);
        })
        .detach();
        self.away(window, cx);
    }

    /// Hands the fix to the System scan page, which is where its exact
    /// commands are read and its output is watched. One thing describes a
    /// fix and one thing runs it.
    fn show_me(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(fix) = self.missing.fix.clone() {
            let what = "Install and update what the machine is missing";
            cx.defer(move |cx| {
                settings::hand_over(what, fix, cx);
                settings::open(Some("scan"), cx);
            });
        }
        self.away(window, cx);
    }

    fn away(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.remove_window();
        cx.defer(|cx| {
            let Some(shared) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return };
            shared.update(cx, |looking, _| looking.open = None);
        });
    }
}

impl Focusable for Prompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// One thing the machine wants, by name.
fn wanted(symbol: &'static str, name: String) -> impl IntoElement {
    rsx! {
        <div class="flex flex-none items-center gap-[9px]" text_size={px(12.)}>
            <div class="flex-none" text_color={theme::text_faint()}>{glyph(symbol, px(15.))}</div>
            <div class="flex-1 min-w-[0px] truncate" text_color={theme::text_dim()}>{name}</div>
        </div>
    }
}

impl Render for Prompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let missing = self.missing.clone();
        rsx! {
            <div
                id="setup"
                class="relative size-full flex items-center justify-center"
                key_context="Setup"
                track_focus={&self.focus}
                font_family={theme::FONT}
                bg={theme::black(0.45)}
                on_action={cx.listener(|prompt: &mut Prompt, _: &Later, window, cx| prompt.later(window, cx))}
            >
                <div
                    class="flex flex-col flex-none gap-[16px] p-[20px] w-[440px]"
                    rounded={px(20.)}
                    bg={theme::pane()}
                    shadow={theme::pane_shadows()}
                    text_color={theme::text()}
                >
                    <div class="flex flex-none items-center gap-[14px]">
                        <div
                            class="flex flex-none items-center justify-center size-[44px] rounded-full"
                            bg={theme::accent()}
                            text_color={theme::on_accent()}
                        >
                            {glyph("handyman", px(24.))}
                        </div>
                        <div class="flex flex-col flex-1 min-w-[0px] gap-[2px]">
                            <div class="flex-none" text_size={px(12.)} text_color={theme::text_dim()}>"The machine wants a hand"</div>
                            <div class="flex-none" text_size={px(16.)} line_height={px(21.)}>{missing.said()}</div>
                        </div>
                    </div>

                    <div class="flex flex-col flex-none gap-[7px] p-[12px]" rounded={px(12.)} bg={theme::white(0.05)}>
                        {for package in missing.packages.iter().cloned() {
                            {wanted("inventory_2", package)}
                        }}
                        {for half in missing.halves.iter().cloned() {
                            {wanted("shield_person", format!("{half} — a newer privileged half is in the checkout"))}
                        }}
                    </div>

                    <div class="flex-none" text_size={px(12.)} line_height={px(17.)} text_color={theme::text_faint()}>
                        "Nothing runs until you have read what it will do. Later keeps this quiet until something new turns up."
                    </div>

                    <div class="flex flex-none items-center gap-[8px]">
                        <div base={chip("Later", false)} id="later" onClick={cx.listener(|prompt: &mut Prompt, _, window, cx| prompt.later(window, cx))} />
                        <div class="flex-1" />
                        <div base={chip("Show me", true)} id="show" onClick={cx.listener(|prompt: &mut Prompt, _, window, cx| prompt.show_me(window, cx))} />
                    </div>
                </div>
            </div>
        }
    }
}

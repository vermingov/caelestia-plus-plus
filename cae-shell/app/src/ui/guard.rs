//! Answering a guard.
//!
//! The firewall and the exploit guard freeze a program and wait for a word
//! from the person at the machine: this is where that word is said. It is
//! the one thing the shell draws that nothing else may cover, because
//! something is stopped until it is answered.

use cae_core::guards::Detail;
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, Global, IntoElement, KeyBinding,
    Render, SharedString, Size, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, actions, div, layer_shell::*, point, prelude::*, px,
};
use serde_json::Value;

use crate::feeds::Feeds;
use crate::theme;
use crate::ui::controls::chip;
use crate::ui::glyph::glyph;
use crate::ui::screen::{self, STRETCH};
use crate::ui::{pointer, rsx};

actions!(guard, [Allow, Once, Block]);

const NAMESPACE: &str = "caelestia-panel";

/// Once, at startup: the three answers, on keys that cannot be pressed by
/// accident while typing somewhere else, since the prompt takes the keyboard.
pub fn bind_keys(cx: &mut App) {
    const CONTEXT: Option<&str> = Some("Guard");
    cx.bind_keys([
        KeyBinding::new("a", Allow, CONTEXT),
        KeyBinding::new("o", Once, CONTEXT),
        KeyBinding::new("b", Block, CONTEXT),
        KeyBinding::new("escape", Block, CONTEXT),
    ]);
}

/// One program waiting on a word, as the daemon describes it.
#[derive(Clone, Debug, PartialEq)]
struct Waiting {
    /// Which daemon is asking: "firewall" or "protection".
    which: String,
    id: i64,
    name: String,
    exe: String,
    parent: String,
    pid: i64,
    detail: String,
    /// Where it was trying to reach, for the firewall.
    reaching: String,
}

impl Waiting {
    fn of(which: &str, ask: &Value) -> Option<Waiting> {
        let word = |name: &str| ask.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        let number = |name: &str| ask.get(name).and_then(Value::as_i64).unwrap_or_default();
        let exe = word("exe");
        let reaching = match (ask.get("dst").and_then(Value::as_str), ask.get("port").and_then(Value::as_i64)) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_string(),
            _ => String::new(),
        };
        Some(Waiting {
            which: which.to_string(),
            id: ask.get("id").and_then(Value::as_i64)?,
            name: if word("name").is_empty() { exe.rsplit('/').next().unwrap_or_default().to_string() } else { word("name") },
            exe,
            parent: word("parent"),
            pid: number("pid"),
            detail: word("detail"),
            reaching,
        })
    }

    fn by_the_firewall(&self) -> bool {
        self.which == "firewall"
    }
}

/// The first thing waiting anywhere, and how many there are in all.
fn waiting(guards: &[Detail]) -> (Option<Waiting>, usize) {
    let all: Vec<Waiting> = guards
        .iter()
        .flat_map(|daemon| daemon.pending.iter().filter_map(|ask| Waiting::of(&daemon.name, ask)))
        .collect();
    (all.first().cloned(), all.len())
}

struct Prompts {
    feeds: Feeds,
    open: Option<WindowHandle<Prompt>>,
    /// What is being answered, so that a second look does not reopen the
    /// same question.
    asking: Option<Waiting>,
}

struct Shared(Entity<Prompts>);

impl Global for Shared {}

/// Puts the question that is waiting back in front of everything, for
/// whatever has just opened over it. A frozen program is not something to
/// leave behind a panel, and the surfaces of a layer lie in the order they
/// were made.
pub fn raise(cx: &mut App) {
    let Some(prompts) = cx.try_global::<Shared>().map(|shared| shared.0.clone()) else { return };
    prompts.update(cx, |prompts, cx| {
        let Some(open) = prompts.open.take() else { return };
        let _ = open.update(cx, |_, window, _| window.remove_window());
        prompts.asking = None;
        prompts.follow(cx);
    });
}

/// Watches the guards, and puts the question in front of the person whenever
/// there is one. For the life of the shell.
pub fn keep(cx: &mut App, feeds: &Feeds) {
    let prompts = cx.new(|cx| {
        cx.observe(&feeds.guards, |prompts: &mut Prompts, _, cx| prompts.follow(cx)).detach();
        Prompts { feeds: feeds.clone(), open: None, asking: None }
    });
    prompts.update(cx, |prompts, cx| prompts.follow(cx));
    cx.set_global(Shared(prompts));
}

impl Prompts {
    fn follow(&mut self, cx: &mut Context<Self>) {
        let (first, _) = waiting(&self.feeds.guards.read(cx).value);
        if first == self.asking {
            // Still the same question, or still none: whatever is on screen
            // is right. The count may have moved, which the prompt redraws
            // for itself.
            return;
        }
        self.asking = first.clone();
        if let Some(open) = self.open.take() {
            let _ = open.update(cx, |_, window, _| window.remove_window());
        }
        let Some(first) = first else { return };

        let (feeds, prompts) = (self.feeds.clone(), cx.weak_entity());
        // Where the person is looking: a question nobody sees is a program
        // that never starts.
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
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                ..Default::default()
            }),
            ..Default::default()
        };
        let opened = cx.open_window(options, move |window, cx| cx.new(|cx| Prompt::new(first, &feeds, prompts, window, cx)));
        self.open = opened.map_err(|error| eprintln!("cae: cannot ask about a frozen program: {error}")).ok();
    }
}

pub struct Prompt {
    asking: Waiting,
    feeds: Feeds,
    prompts: gpui::WeakEntity<Prompts>,
    focus: FocusHandle,
}

impl Prompt {
    fn new(asking: Waiting, feeds: &Feeds, prompts: gpui::WeakEntity<Prompts>, window: &mut Window, cx: &mut Context<Self>) -> Prompt {
        cx.observe(&feeds.guards, |_, _, cx| cx.notify()).detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Prompt { asking, feeds: feeds.clone(), prompts, focus }
    }

    /// Says the word, and takes the question away: the next one, if there is
    /// one, is put up by the watcher.
    fn answer(&mut self, action: &'static str, remember: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (which, id) = (self.asking.which.clone(), self.asking.id);
        let watcher = self.feeds.watcher.clone();
        cx.background_spawn(async move { watcher.verdict(&which, id, action, remember) }).detach();
        let prompts = self.prompts.clone();
        window.remove_window();
        cx.defer(move |cx| {
            let _ = prompts.update(cx, |prompts, cx| {
                prompts.open = None;
                prompts.asking = None;
                prompts.follow(cx);
            });
        });
    }
}

impl Focusable for Prompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// One line of the detail: what it is, and what it says.
fn line(label: &'static str, value: impl Into<SharedString>) -> AnyElement {
    let value: SharedString = value.into();
    if value.is_empty() {
        return div().into_any_element();
    }
    rsx! {
        <div class="flex flex-none items-baseline gap-[10px]" text_size={px(12.)}>
            <div class="flex-none w-[86px]" text_color={theme::text_faint()}>{label}</div>
            <div class="flex-1 min-w-[0px] truncate" text_color={theme::text_dim()}>{value}</div>
        </div>
    }
    .into_any_element()
}

impl Render for Prompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let asking = self.asking.clone();
        let (_, waiting_now) = waiting(&self.feeds.guards.read(cx).value);
        let firewall = asking.by_the_firewall();
        let heading = if firewall { "Connection request" } else { "Program frozen" };
        let said = if asking.detail.is_empty() {
            if firewall { "Something it has not done before".to_string() } else { asking.detail.clone() }
        } else {
            asking.detail.clone()
        };

        rsx! {
            <div
                id="guard"
                class="relative size-full flex items-center justify-center"
                key_context="Guard"
                track_focus={&self.focus}
                font_family={theme::FONT}
                bg={theme::black(0.45)}
                on_action={cx.listener(|prompt: &mut Prompt, _: &Allow, window, cx| prompt.answer("allow", true, window, cx))}
                on_action={cx.listener(|prompt: &mut Prompt, _: &Once, window, cx| prompt.answer("allow", false, window, cx))}
                on_action={cx.listener(|prompt: &mut Prompt, _: &Block, window, cx| prompt.answer("deny", true, window, cx))}
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
                            {glyph(if firewall { "gpp_maybe" } else { "gpp_bad" }, px(24.))}
                        </div>
                        <div class="flex flex-col flex-1 min-w-[0px] gap-[2px]">
                            <div class="flex-none" text_size={px(12.)} text_color={theme::text_dim()}>{heading}</div>
                            <div class="flex-none truncate" text_size={px(16.)}>{asking.name.clone()}</div>
                        </div>
                    </div>

                    {...(!said.is_empty()).then(|| rsx! {
                        <div class="flex-none" text_size={px(12.5)} text_color={theme::text_dim()}>{said}</div>
                    })}

                    <div
                        class="flex flex-col flex-none gap-[6px] p-[12px]"
                        rounded={px(12.)}
                        bg={theme::white(0.05)}
                    >
                        {line("Reaching", asking.reaching.clone())}
                        {line("Path", asking.exe.clone())}
                        {line("Started by", asking.parent.clone())}
                        {line("Process", if asking.pid > 0 { asking.pid.to_string() } else { String::new() })}
                    </div>

                    <div class="flex flex-none items-center gap-[8px]">
                        <div
                            base={chip("Block", false)}
                            id="block"
                            onClick={cx.listener(|prompt: &mut Prompt, _, window, cx| prompt.answer("deny", true, window, cx))}
                        />
                        <div
                            base={chip("Allow once", false)}
                            id="once"
                            onClick={cx.listener(|prompt: &mut Prompt, _, window, cx| prompt.answer("allow", false, window, cx))}
                        />
                        <div class="flex-1" />
                        <div
                            base={chip("Allow", true)}
                            id="allow"
                            onClick={cx.listener(|prompt: &mut Prompt, _, window, cx| prompt.answer("allow", true, window, cx))}
                        />
                    </div>

                    {...(waiting_now > 1).then(|| rsx! {
                        <div class="flex-none" text_size={px(11.5)} text_color={theme::text_faint()}>
                            {format!("{} more waiting", waiting_now - 1)}
                        </div>
                    })}
                </div>
                {pointer::see_out()}
            </div>
        }
    }
}

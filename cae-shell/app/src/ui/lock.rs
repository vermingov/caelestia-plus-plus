//! The lock screen.
//!
//! Not a panel over the desktop: a session lock. The compositor is asked to
//! stop showing everything else (`ext-session-lock-v1`), and it goes on
//! hiding it even if this process dies — which is the whole point, and the
//! reason the window kind behind it had to be added to GPUI.
//!
//! One window per screen, because a screen with no lock surface on it is
//! shown by the compositor as a blank. The one the pointer is on has the
//! password box; the others have the time and the wallpaper, so a second
//! screen is not a black rectangle.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use cae_core::unlocking;
use gpui::{
    App, AppContext, Bounds, Context, DisplayId, Entity, FocusHandle, Focusable, Global, IntoElement, KeyBinding,
    ObjectFit, Render, Size, Styled, Task, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, actions, div, img, point, prelude::*, px, session_lock::SessionLockOptions,
};

use crate::feeds::Feeds;
use crate::ui::field::Field;
use crate::ui::glyph::glyph;
use crate::ui::screen::{self, STRETCH};
use crate::ui::rsx;
use crate::{clock, theme};

actions!(lock, [Unlock]);

/// How long a wrong password is said to be wrong for.
const SAID_WRONG: Duration = Duration::from_secs(4);

/// Once, at startup.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("enter", Unlock, Some("Lock"))]);
}

/// The windows that are up, one per screen. Holding them is what keeps the
/// session locked: letting go unlocks it.
#[derive(Default)]
struct Locked(HashMap<DisplayId, WindowHandle<Face>>);

impl Global for Locked {}

/// Read without touching: taking the global mutably tells everything that
/// observes it that it changed, and an observer that asks would hear itself.
pub fn is_locked(cx: &App) -> bool {
    cx.try_global::<Locked>().is_some_and(|locked| !locked.0.is_empty())
}

/// Calls `changed` each time the session is locked or unlocked.
pub fn observe(cx: &mut App, mut changed: impl FnMut(&mut App) + 'static) {
    cx.observe_global::<Locked>(move |cx| changed(cx)).detach();
}

/// What Quickshell is asked about before this is drawn: two session locks
/// is one too many, and the compositor takes only the first.
const PIECE: &str = "lock";

/// Locks it. Nothing if it already is. While the lock is still Quickshell's,
/// Quickshell is asked instead.
pub fn lock(cx: &mut App, feeds: &Feeds) {
    if is_locked(cx) {
        return;
    }
    let feeds = feeds.clone();
    crate::ours::when_known(PIECE, cx, move |ours, cx| {
        if ours {
            return lock_ours(&feeds, cx);
        }
        cx.background_spawn(async { drop(cae_core::services::ipc("lock", "lock", &[])) }).detach();
    });
}

fn lock_ours(feeds: &Feeds, cx: &mut App) {
    if is_locked(cx) {
        return;
    }
    // Which screen the person is at, so the box is where they are looking.
    let asking_on = screen::focused_display(cx);
    let wallpaper = cae_core::launcher::wallpapers::current().map(PathBuf::from).filter(|picture| picture.is_file());

    let mut up = HashMap::new();
    for (name, display) in screen::outputs(cx) {
        let options = WindowOptions {
            titlebar: None,
            display_id: Some(display),
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), Size::new(STRETCH, STRETCH)))),
            app_id: Some("caelestia-lock".to_string()),
            window_background: WindowBackgroundAppearance::Opaque,
            kind: WindowKind::SessionLock(SessionLockOptions::default()),
            ..crate::ui::surface::options()
        };
        let asking = asking_on.is_none_or(|focused| focused == display) && up.is_empty();
        let (feeds, wallpaper) = (feeds.clone(), wallpaper.clone());
        let opened = cx.open_window(options, move |window, cx| cx.new(|cx| Face::new(asking, wallpaper, &feeds, window, cx)));
        match opened {
            Ok(face) => drop(up.insert(display, face)),
            Err(error) => eprintln!("cae: cannot lock {name}: {error}"),
        }
    }
    if up.is_empty() {
        return eprintln!("cae: nothing could be locked, so the session is not");
    }
    cx.set_global(Locked(up));
}

/// Unlocks it: the surfaces go, and the compositor shows the desktop again.
pub fn unlock(cx: &mut App) {
    for (_, face) in std::mem::take(&mut cx.default_global::<Locked>().0) {
        let _ = face.update(cx, |_, window, _| window.remove_window());
    }
}

/// What one screen shows while it is locked.
pub struct Face {
    /// Whether this is the screen with the password box on it.
    asking: bool,
    wallpaper: Option<PathBuf>,
    feeds: Feeds,
    password: Entity<Field>,
    /// Set while PAM is thinking, which is a second or two for a wrong one.
    checking: bool,
    wrong: bool,
    focus: FocusHandle,
    _ticking: Task<()>,
}

impl Face {
    fn new(asking: bool, wallpaper: Option<PathBuf>, feeds: &Feeds, window: &mut Window, cx: &mut Context<Self>) -> Face {
        let password = cx.new(|cx| Field::new("Password", cx).secret());
        if asking {
            window.focus(&password.focus_handle(cx), cx);
        }
        cx.observe(&feeds.media, |_, _, cx| cx.notify()).detach();

        // The minute turns while it is up.
        let ticking = cx.spawn(async move |face, cx| {
            loop {
                cx.background_executor().timer(clock::until_next_minute()).await;
                if face.update(cx, |_: &mut Face, cx| cx.notify()).is_err() {
                    return;
                }
            }
        });

        Face {
            asking,
            wallpaper,
            feeds: feeds.clone(),
            password,
            checking: false,
            wrong: false,
            focus: cx.focus_handle(),
            _ticking: ticking,
        }
    }

    /// Asks whether that was the password. Away from the thread that draws:
    /// PAM takes its time over a wrong one on purpose.
    fn try_it(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        let said = self.password.read(cx).text().to_string();
        if said.is_empty() {
            return;
        }
        self.password.update(cx, |field, cx| field.set_text("", cx));
        (self.checking, self.wrong) = (true, false);
        cx.notify();

        cx.spawn(async move |face, cx| {
            let right = cx.background_spawn(async move { unlocking::accepted(&said) }).await;
            let _ = face.update(cx, |face: &mut Face, cx| {
                face.checking = false;
                if right {
                    return cx.defer(unlock);
                }
                face.wrong = true;
                cx.notify();
                // The word goes again after a while, so the screen does not
                // keep saying it at somebody who has walked away.
                cx.spawn(async move |face, cx| {
                    cx.background_executor().timer(SAID_WRONG).await;
                    let _ = face.update(cx, |face: &mut Face, cx| {
                        face.wrong = false;
                        cx.notify();
                    });
                })
                .detach();
            });
        })
        .detach();
    }
}

impl Focusable for Face {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.asking { self.password.focus_handle(cx) } else { self.focus.clone() }
    }
}

impl Render for Face {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (date, time) = clock::now();
        let playing = self.feeds.media.read(cx).value.clone().filter(|now| !now.title.is_empty());
        let said = if self.checking {
            "Asking…"
        } else if self.wrong {
            "That is not the password"
        } else {
            ""
        };

        rsx! {
            <div
                id="lock"
                class="relative size-full flex flex-col items-center justify-center gap-[18px]"
                key_context="Lock"
                track_focus={&self.focus}
                font_family={theme::FONT}
                bg={theme::black(1.)}
                text_color={theme::text()}
                on_action={cx.listener(|face: &mut Face, _: &Unlock, _, cx| face.try_it(cx))}
            >
                {...self.wallpaper.clone().map(|picture| rsx! {
                    <img class="absolute size-full" src={picture} object_fit={ObjectFit::Cover} />
                })}
                // Over the picture, so the time can be read whatever it is
                // of, and so that a photograph is not the brightest thing in
                // a dark room at three in the morning.
                <div class="absolute size-full" bg={theme::black(0.72)} />

                <div class="relative flex flex-col items-center gap-[2px]">
                    <div text_size={px(88.)} line_height={px(96.)}>{time}</div>
                    <div text_size={px(15.)} text_color={theme::text_dim()}>{date}</div>
                </div>

                {...playing.map(|now| rsx! {
                    <div
                        class="relative flex items-center gap-[9px] py-[7px] px-[13px]"
                        rounded={px(999.)}
                        bg={theme::white(0.07)}
                        text_size={px(12.)}
                        text_color={theme::text_dim()}
                    >
                        {glyph("music_note", px(15.))}
                        <div class="max-w-[420px] truncate">
                            {if now.artist.is_empty() { now.title.clone() } else { format!("{} — {}", now.artist, now.title) }}
                        </div>
                    </div>
                })}

                {...self.asking.then(|| rsx! {
                    <div class="relative flex flex-col items-center gap-[10px]">
                        <div
                            class="flex items-center gap-[9px] h-[38px] w-[300px] px-[14px]"
                            rounded={px(999.)}
                            bg={theme::white(0.09)}
                            shadow={theme::edge(0.08)}
                            text_size={px(13.)}
                            when={(self.wrong, |box_| box_.bg(theme::alert().opacity(0.18)))}
                        >
                            {glyph(if self.checking { "hourglass" } else { "lock" }, px(16.))}
                            <div class="flex-1 min-w-[0px]">{self.password.clone()}</div>
                        </div>
                        <div class="h-[16px]" text_size={px(11.5)} text_color={if self.wrong { theme::alert() } else { theme::text_faint() }}>
                            {said}
                        </div>
                    </div>
                })}
            </div>
        }
    }
}

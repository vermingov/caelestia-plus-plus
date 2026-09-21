//! What the desktop is doing, kept current.
//!
//! The backend is threads that block: on Hyprland's event socket, on a pipe
//! of audio, on D-Bus. Each is handed a callback and calls it when there is
//! something new. GPUI's state lives on one thread, so the callback only puts
//! the value on a channel, and a task on that thread moves it into the entity
//! the views are watching. Nothing is polled from the drawing side.

use std::time::{Duration, Instant};

use cae_core::{guards, hypr, logo, media, notifs, services, spectrum, system, tray, volume};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, AppContext, Entity, Global};

/// The latest value of one thing the shell shows.
pub struct Feed<T> {
    pub value: T,
}

/// The bar's own settings, which are three reads of the same two files.
#[derive(Clone, PartialEq)]
pub struct Settings {
    pub logo: logo::Logo,
    pub workspaces: logo::BarConfig,
    pub layout: logo::Layout,
}

impl Settings {
    pub fn read() -> Settings {
        Settings { logo: logo::read(), workspaces: logo::bar_config(), layout: logo::layout() }
    }
}

/// One frame of the visualiser.
#[derive(Clone, Default)]
pub struct Spectrum {
    pub bars: Vec<u8>,
    pub live: bool,
}

/// Every feed, as handles. Cloning is cloning handles: there is one of each.
#[derive(Clone)]
pub struct Feeds {
    pub hypr: Entity<Feed<hypr::State>>,
    pub system: Entity<Feed<system::Snapshot>>,
    pub services: Entity<Feed<services::Snapshot>>,
    pub tray: Entity<Feed<Vec<tray::Item>>>,
    pub media: Entity<Feed<Option<media::NowPlaying>>>,
    pub spectrum: Entity<Feed<Spectrum>>,
    pub settings: Entity<Feed<Settings>>,
    /// Every notification there is, newest first, and what state the centre
    /// and do-not-disturb are in. The bell reads a summary of it.
    pub notifs: Entity<Feed<notifs::Feed>>,
    /// The server itself, for whatever a notification is told to do. A shell
    /// that is only being looked at has one that serves nobody; see `start`.
    pub server: notifs::Notifs,
    /// Whether this shell is the desktop's notification server.
    pub serving: bool,
    /// What the two guard daemons are saying, in full: the counts are in
    /// `services`, and this is what the prompts and the security centre are
    /// drawn from. Its own feed because it changes when a daemon speaks
    /// rather than on a tick, and a frozen program is waiting.
    pub guards: Entity<Feed<Vec<guards::Detail>>>,
    /// The guards themselves, for saying a word back to one.
    pub watcher: guards::Watcher,
}

/// Where anything that opens something of its own finds them: a window opened
/// from a button on a panel is not handed down a tree the way a bar is.
impl Global for Feeds {}

/// How often the readouts are re-read. A second is the rate at which a CPU
/// percentage means anything.
const TICK: Duration = Duration::from_secs(1);

/// The slow tick. Everything on it is a question put to somebody else, so it
/// runs at the rate a person would notice a stale answer at.
const SLOW_TICK: Duration = Duration::from_secs(5);

/// How often the shell looks for a newer release of itself. Rarely: it is a
/// fetch over the network, and a release is a thing that happens on a good
/// day, not on a good minute.
const UPDATE_CHECK: Duration = Duration::from_secs(3 * 60 * 60);

/// Starts a thread that produces values and lands each one in `entity`.
///
/// `newest_only` is for a producer that outruns the screen: whatever piled up
/// while a frame was being drawn is skipped, and only the last one is shown.
fn pump<T: Send + 'static>(
    cx: &mut App,
    entity: &Entity<Feed<T>>,
    newest_only: bool,
    produce: impl FnOnce(UnboundedSender<T>) + Send + 'static,
) {
    let (tx, mut rx) = unbounded::<T>();
    std::thread::spawn(move || produce(tx));

    let entity = entity.downgrade();
    cx.spawn(async move |cx| {
        while let Some(mut value) = rx.next().await {
            if newest_only {
                while let Ok(newer) = rx.try_recv() {
                    value = newer;
                }
            }
            let landed = entity.update(cx, |feed, cx| {
                feed.value = value;
                cx.notify();
            });
            if landed.is_err() {
                break;
            }
        }
    })
    .detach();
}

impl Feeds {
    /// `serving` is whether this process is the desktop's notification
    /// server. A shell run beside the real one, to look at it, must not be:
    /// there is one bus name and one socket, and taking either would take it
    /// from the shell that is actually in use.
    pub fn start(cx: &mut App, serving: bool) -> Feeds {
        let watcher = guards::Watcher::start();
        let server = if serving { notifs::start() } else { notifs::Notifs::new() };

        let feeds = Feeds {
            hypr: cx.new(|_| Feed { value: hypr::read_state() }),
            system: cx.new(|_| Feed { value: system::Snapshot::default() }),
            services: cx.new(|_| Feed { value: services::read(watcher.read()) }),
            tray: cx.new(|_| Feed { value: tray::items() }),
            media: cx.new(|_| Feed { value: media::now() }),
            spectrum: cx.new(|_| Feed { value: Spectrum::default() }),
            settings: cx.new(|_| Feed { value: Settings::read() }),
            notifs: cx.new(|_| Feed { value: server.feed() }),
            server: server.clone(),
            serving,
            guards: cx.new(|_| Feed { value: watcher.detail() }),
            watcher: watcher.clone(),
        };

        pump(cx, &feeds.hypr, false, |tx| hypr::watch(move |state| drop(tx.unbounded_send(state))));
        pump(cx, &feeds.tray, false, |tx| tray::watch(move |items| drop(tx.unbounded_send(items))));
        pump(cx, &feeds.media, false, |tx| media::watch(move |now| drop(tx.unbounded_send(now))));
        pump(cx, &feeds.spectrum, true, |tx| {
            spectrum::watch(move |bars, live| drop(tx.unbounded_send(Spectrum { bars, live })))
        });
        pump(cx, &feeds.system, false, sample_system);
        pump(cx, &feeds.guards, true, {
            let watcher = watcher.clone();
            move |tx| {
                let asking = watcher.clone();
                let told = tx.clone();
                watcher.on_change(move || drop(told.unbounded_send(asking.detail())));
                // Whatever the daemons said while this was being set up: they
                // connect as the shell starts, and a prompt that arrived in
                // that moment must not wait for the next one.
                let _ = tx.unbounded_send(watcher.detail());
                // The callback is the feed: this thread has nothing else to
                // do, and going would drop what it holds.
                loop {
                    std::thread::park();
                }
            }
        });
        pump(cx, &feeds.services, false, move |tx| sample_services(tx, watcher));
        pump(cx, &feeds.settings, false, watch_settings);
        // Nothing draws this: it says so once, the way the shell it replaced
        // did, and the settings show what there is to know.
        if serving {
            std::thread::spawn(watch_for_a_release);
        }
        pump(cx, &feeds.notifs, false, move |tx| {
            server.on_change(move |feed| drop(tx.unbounded_send(feed.clone())));
        });

        feeds
    }
}

/// The readouts, once a second, and the volume the moment it moves.
fn sample_system(tx: UnboundedSender<system::Snapshot>) {
    let (moved, moves) = std::sync::mpsc::channel();
    std::thread::spawn(move || volume::watch(|levels| drop(moved.send(levels))));

    let mut sampler = system::Sampler::new();
    let mut levels = volume::Levels::default();
    let mut shown: Option<system::Snapshot> = None;
    let mut due = Instant::now() + TICK;
    loop {
        let snapshot = match moves.recv_timeout(due.saturating_duration_since(Instant::now())) {
            // Only the volume is new, and the rest stays as it was read: a
            // CPU percentage averaged over the moment since a volume key was
            // pressed is noise.
            Ok(now) => {
                levels = now;
                let Some(shown) = &shown else { continue };
                system::Snapshot {
                    volume: levels.volume.clone(),
                    microphone: levels.microphone.clone(),
                    ..shown.clone()
                }
            }
            Err(_) => {
                // A watcher that has gone lands here at once rather than on
                // time, and must not turn the tick into a spin.
                std::thread::sleep(due.saturating_duration_since(Instant::now()));
                due = Instant::now() + TICK;
                sampler.sample(&levels)
            }
        };
        // Nothing to draw differently, nothing to wake the bar for. At rest
        // this is most of the ticks.
        if shown.as_ref() == Some(&snapshot) {
            continue;
        }
        shown = Some(snapshot.clone());
        if tx.unbounded_send(snapshot).is_err() {
            return;
        }
    }
}

/// The slow feed: power profile, the two guards, feature modes, bluetooth.
fn sample_services(tx: UnboundedSender<services::Snapshot>, watcher: guards::Watcher) {
    let mut last = None;
    loop {
        let snapshot = services::read(watcher.read());
        if last.as_ref() != Some(&snapshot) {
            last = Some(snapshot.clone());
            if tx.unbounded_send(snapshot).is_err() {
                return;
            }
        }
        std::thread::sleep(SLOW_TICK);
    }
}

/// Says once, when a release appears, that there is one. Only once for each:
/// a shell that mentioned it every three hours would be a nag.
fn watch_for_a_release() {
    let mut mentioned = String::new();
    loop {
        // Not at startup: the session has enough to do, and the checkout was
        // just as old a minute ago.
        std::thread::sleep(UPDATE_CHECK);
        let Ok(found) = cae_core::updates::check() else { continue };
        if found.behind == 0 || found.release == mentioned {
            continue;
        }
        mentioned = found.release.clone();
        let behind = found.behind;
        cae_core::tell::said(
            &format!("{} is out", found.release),
            &format!("{behind} change{} since this one. Settings, then Updates.", if behind == 1 { "" } else { "s" }),
            "upgrade",
        );
    }
}

/// Re-reads the settings when somebody changes them, so that turning the logo
/// off in Settings takes it off the bar without restarting anything.
fn watch_settings(tx: UnboundedSender<Settings>) {
    let mut last = logo::stamp();
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let now = logo::stamp();
        if now == last {
            continue;
        }
        last = now;
        if tx.unbounded_send(Settings::read()).is_err() {
            return;
        }
    }
}

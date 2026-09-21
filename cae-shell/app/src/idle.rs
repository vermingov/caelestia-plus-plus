//! Noticing that nobody is there.
//!
//! The compositor knows how long it has been since anybody touched anything,
//! and says so through `ext_idle_notify_v1` — which GPUI does not speak, so
//! this is a connection of its own on a thread of its own, as the background
//! and the inhibitor are. What is done about it is not decided here: the
//! thread says only that a timeout has passed, or that somebody is back.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cae_core::{idling, logind};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, AppContext, AsyncApp, Context, Entity};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::ext::idle_notify::v1::client::{ext_idle_notification_v1, ext_idle_notifier_v1};

use crate::feeds::Feeds;
use crate::ours;

/// One length of quiet worth being told about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct After {
    pub quiet: Duration,
    /// Whether something asking to be left alone — a film, the switch in the
    /// utilities — stops it counting.
    pub respect_inhibitors: bool,
}

/// What the compositor said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    /// The `at`th of the lengths it was told to watch has passed.
    Quiet(usize),
    /// Somebody is back, after the `at`th length had passed.
    Back(usize),
}

struct Told {
    watching: Vec<After>,
    stop: bool,
}

/// The compositor is watched for as long as this is kept.
pub struct Watch {
    told: Arc<Mutex<Told>>,
    wake: UnixStream,
}

impl Watch {
    /// Starts watching, and says what happens down `moments`.
    pub fn start(watching: Vec<After>, moments: UnboundedSender<Moment>) -> std::io::Result<Watch> {
        let (wake, woken) = UnixStream::pair()?;
        woken.set_nonblocking(true)?;
        let told = Arc::new(Mutex::new(Told { watching, stop: false }));
        let heard = told.clone();
        std::thread::Builder::new().name("idle".to_string()).spawn(move || {
            if let Err(error) = run(&heard, woken, &moments) {
                eprintln!("cae: nothing is watching for idleness any more: {error}");
            }
        })?;
        Ok(Watch { told, wake })
    }

    /// Watches these lengths instead, which is how a rule that says not to
    /// count just now is applied: the length is simply not watched.
    pub fn watch(&self, watching: Vec<After>) {
        self.tell(|told| told.watching = watching);
    }

    fn tell(&self, change: impl FnOnce(&mut Told)) {
        change(&mut self.told.lock().unwrap_or_else(PoisonError::into_inner));
        let _ = (&self.wake).write(&[0]);
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.tell(|told| told.stop = true);
    }
}

/// What the thread keeps: the notifications it has asked for, and what it
/// asked for them about.
#[derive(Default)]
struct Watching {
    asked: Vec<(After, ext_idle_notification_v1::ExtIdleNotificationV1)>,
    /// Filled as events arrive, and emptied by the loop below: a dispatch
    /// may not touch the channel the loop owns.
    heard: Vec<Moment>,
}

impl Watching {
    /// Asks for one notification per length, and lets go of the ones that are
    /// no longer wanted. A length that is already watched is left alone, so
    /// that a rule changing elsewhere does not restart its clock.
    fn follow(&mut self, wanted: &[After], notifier: &ext_idle_notifier_v1::ExtIdleNotifierV1, seat: &wl_seat::WlSeat, queue: &QueueHandle<Watching>) {
        self.asked.retain(|(after, notification)| {
            let stays = wanted.contains(after);
            if !stays {
                notification.destroy();
            }
            stays
        });
        for (at, after) in wanted.iter().enumerate() {
            if self.asked.iter().any(|(known, _)| known == after) {
                continue;
            }
            let quiet = after.quiet.as_millis().min(u128::from(u32::MAX)) as u32;
            let notification = if after.respect_inhibitors {
                notifier.get_idle_notification(quiet, seat, queue, at)
            } else {
                notifier.get_input_idle_notification(quiet, seat, queue, at)
            };
            self.asked.push((*after, notification));
        }
    }
}

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, usize> for Watching {
    fn event(
        watching: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        at: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        watching.heard.push(match event {
            ext_idle_notification_v1::Event::Idled => Moment::Quiet(*at),
            ext_idle_notification_v1::Event::Resumed => Moment::Back(*at),
            _ => return,
        });
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Watching {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_noop!(Watching: ext_idle_notifier_v1::ExtIdleNotifierV1);
delegate_noop!(Watching: ignore wl_seat::WlSeat);

fn run(told: &Mutex<Told>, mut woken: UnixStream, moments: &UnboundedSender<Moment>) -> Result<(), String> {
    let connection = Connection::connect_to_env().map_err(|error| error.to_string())?;
    let (globals, mut events) = registry_queue_init::<Watching>(&connection).map_err(|error| error.to_string())?;
    let queue = events.handle();
    let missing = |error: wayland_client::globals::BindError| format!("the compositor lacks something: {error}");
    // Version 2 is what has the notification that ignores inhibitors; the
    // lengths that would have used it make do with one that does not.
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = globals.bind(&queue, 1..=2, ()).map_err(missing)?;
    let seat: wl_seat::WlSeat = globals.bind(&queue, 1..=9, ()).map_err(missing)?;
    let ignores_inhibitors = notifier.version() >= 2;

    let mut watching = Watching::default();
    loop {
        events.dispatch_pending(&mut watching).map_err(|error| error.to_string())?;
        for moment in watching.heard.drain(..) {
            if moments.unbounded_send(moment).is_err() {
                return Ok(());
            }
        }
        let wanted: Vec<After> = {
            let told = told.lock().unwrap_or_else(PoisonError::into_inner);
            if told.stop {
                return Ok(());
            }
            told.watching
                .iter()
                .map(|after| After { respect_inhibitors: after.respect_inhibitors || !ignores_inhibitors, ..*after })
                .collect()
        };
        watching.follow(&wanted, &notifier, &seat, &queue);
        wait(&connection, &events, &mut woken)?;
    }
}

/// Sleeps until the compositor says something or somebody writes to `woken`.
fn wait(connection: &Connection, events: &wayland_client::EventQueue<Watching>, woken: &mut UnixStream) -> Result<(), String> {
    connection.flush().map_err(|error| error.to_string())?;
    let Some(reading) = events.prepare_read() else { return Ok(()) };

    let listen = |fd: i32| libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    let mut heard = [listen(reading.connection_fd().as_raw_fd()), listen(woken.as_raw_fd())];
    // SAFETY: two descriptors that are open for as long as the call.
    unsafe { libc::poll(heard.as_mut_ptr(), heard.len() as libc::nfds_t, -1) };

    let [compositor, waker] = heard.map(|fd| fd.revents);
    if compositor & (libc::POLLERR | libc::POLLHUP) != 0 {
        return Err("the compositor has gone".to_string());
    }
    if compositor & libc::POLLIN != 0 {
        reading.read().map_err(|error| error.to_string())?;
    }
    if waker != 0 {
        let mut written = [0; 64];
        while woken.read(&mut written).is_ok_and(|read| read == written.len()) {}
    }
    Ok(())
}

/// What Quickshell is asked about before any of this is acted on: two
/// shells watching the same quiet would lock the machine twice.
const PIECE: &str = "idle";

/// What the login manager says, which is the same three things the idle
/// timeouts ask for: lock, unlock, and lock before the machine sleeps.
fn listen_to_logind(cx: &mut App, feeds: &Feeds) {
    let (told, heard) = unbounded();
    std::thread::Builder::new()
        .name("logind".to_string())
        .spawn(move || logind::watch(|said| drop(told.unbounded_send(said))))
        .ok();

    let feeds = feeds.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut heard = heard;
        while let Some(said) = heard.next().await {
            let feeds = feeds.clone();
            cx.update(|cx| match said {
                logind::Said::Lock => crate::ui::lock::lock(cx, &feeds),
                logind::Said::Unlock => crate::ui::lock::unlock(cx),
                logind::Said::Sleeping if idling::Settings::read().lock_before_sleep => {
                    crate::ui::lock::lock(cx, &feeds);
                }
                logind::Said::Sleeping => {}
            });
        }
    })
    .detach();
}

/// Watches for quiet and does what the settings say about it, for the life
/// of the shell.
///
/// The lengths that are watched are only the ones that count just now: one
/// that is not to run while something is playing is simply not asked for,
/// which is also how its clock is reset when the music stops.
pub fn keep(cx: &mut App, feeds: &Feeds) {
    let (moments, heard) = unbounded();
    let policy: Entity<Policy> = cx.new(|cx| {
        cx.observe(&feeds.settings, |policy: &mut Policy, _, cx| {
            policy.settings = idling::Settings::read();
            policy.follow(cx);
        })
        .detach();
        cx.observe(&feeds.media, |policy: &mut Policy, _, cx| policy.follow(cx)).detach();
        cx.observe(&feeds.system, |policy: &mut Policy, _, cx| policy.follow(cx)).detach();
        Policy { feeds: feeds.clone(), settings: idling::Settings::read(), watch: None, watching: Vec::new(), moments }
    });
    policy.update(cx, |policy, cx| policy.follow(cx));
    listen_to_logind(cx, feeds);

    // Quickshell is asked again every so often whether the watching is ours
    // yet, and nothing above happens on a desktop nobody is at to ask it.
    let asking = policy.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            asking.update(cx, |policy: &mut Policy, cx| policy.follow(cx));
            cx.background_executor().timer(Duration::from_secs(5)).await;
        }
    })
    .detach();

    // The loop holds the policy, which is what keeps it for the life of the
    // shell.
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut heard = heard;
        while let Some(moment) = heard.next().await {
            policy.update(cx, |policy: &mut Policy, cx| policy.happened(moment, cx));
        }
    })
    .detach();
}

struct Policy {
    feeds: Feeds,
    settings: idling::Settings,
    watch: Option<Watch>,
    /// The lengths being watched, in the order the watcher knows them by.
    watching: Vec<(usize, After)>,
    moments: UnboundedSender<Moment>,
}

impl Policy {
    /// Tells the watcher which lengths count just now.
    fn follow(&mut self, cx: &mut Context<Self>) {
        if !ours::is_ours(PIECE, cx) {
            // Still the old shell's to watch. Nothing is asked for, so
            // nothing is heard.
            (self.watch, self.watching) = (None, Vec::new());
            return;
        }
        let playing = self.feeds.media.read(cx).value.as_ref().is_some_and(|now| now.playing);
        let charging = self.feeds.system.read(cx).value.battery.as_ref().is_none_or(|battery| battery.on_mains);
        let counts = |timeout: &idling::Timeout| {
            timeout.enabled && !(timeout.not_while_playing && playing) && !(timeout.not_while_charging && charging)
        };
        let watching: Vec<(usize, After)> = self
            .settings
            .timeouts
            .iter()
            .enumerate()
            .filter(|(_, timeout)| counts(timeout))
            .map(|(at, timeout)| (at, After { quiet: timeout.after, respect_inhibitors: timeout.respect_inhibitors }))
            .collect();
        if self.watching == watching && self.watch.is_some() {
            return;
        }
        self.watching = watching;
        let lengths: Vec<After> = self.watching.iter().map(|(_, after)| *after).collect();
        match &self.watch {
            Some(watch) => watch.watch(lengths),
            None => {
                self.watch = Watch::start(lengths, self.moments.clone())
                    .map_err(|error| eprintln!("cae: cannot watch for idleness: {error}"))
                    .ok();
            }
        }
    }

    /// A length has passed, or somebody is back.
    fn happened(&mut self, moment: Moment, cx: &mut Context<Self>) {
        let (at, going) = match moment {
            Moment::Quiet(at) => (at, true),
            Moment::Back(at) => (at, false),
        };
        // The watcher counts the lengths it was given; which of the settings'
        // that is depends on which were being watched at the time.
        let Some((which, _)) = self.watching.get(at) else { return };
        let Some(timeout) = self.settings.timeouts.get(*which) else { return };
        let what = if going { timeout.idle.clone() } else { timeout.back.clone() };
        let Some(what) = what else { return };
        cx.background_spawn(async move { idling::run(&what) }).detach();
    }
}

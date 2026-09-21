//! What the settings window shows, and how any of it is changed.
//!
//! Three places hold a setting: the two files the shells share, and the
//! compositor's config, which a helper edits. A row does not care which. It
//! names a `Source`, reads it here, and sets it here; this shows the new
//! value at once and sees it written in the background, in order.

use std::sync::mpsc;
use std::time::Duration;

use cae_core::{config, hyprmod, logo};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{AppContext, Context, SharedString};
use serde_json::Value;

use crate::feeds::{self, Feeds};

/// Where one setting is kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    File(config::Key),
    /// One of the compositor's curated knobs, by the helper's name for it.
    Knob(&'static str),
}

pub const fn shell(path: &'static str) -> Source {
    Source::File(config::shell(path))
}

pub const fn prefs(path: &'static str) -> Source {
    Source::File(config::prefs(path))
}

pub const fn knob(name: &'static str) -> Source {
    Source::Knob(name)
}

/// One change waiting to be made outside this process.
struct Job {
    /// Jobs with the same key replace one another while they wait. A stepper
    /// pressed six times is six jobs, and only where it ended up matters.
    key: String,
    work: Box<dyn FnOnce() -> Result<(), String> + Send>,
    /// A file the bar reads was written, so the bar's settings are read again
    /// rather than left to notice a couple of seconds later.
    rereads_bar: bool,
}

struct Done {
    failure: Option<String>,
    bar: Option<feeds::Settings>,
}

/// How often the files are checked for somebody else's changes: the QML
/// shell has switches of its own that write the same keys.
const LOOK: Duration = Duration::from_secs(2);

pub struct Store {
    files: config::Snapshot,
    /// Nothing until the helper has answered, and nothing for good on a
    /// machine whose Hyprland config the helper does not know.
    knobs: Value,
    knobs_asked: bool,
    /// What went wrong with the last change, for the window to say.
    pub failure: Option<SharedString>,
    jobs: mpsc::Sender<Job>,
    waiting: usize,
}

impl Store {
    pub fn new(feeds: &Feeds, cx: &mut Context<Self>) -> Store {
        let (jobs, queued) = mpsc::channel::<Job>();
        let (finished, mut done) = unbounded::<Done>();
        std::thread::spawn(move || work_through(queued, finished));

        let feeds = feeds.clone();
        cx.spawn(async move |store, cx| {
            while let Some(done) = done.next().await {
                let landed = store.update(cx, |store: &mut Store, cx| {
                    store.waiting = store.waiting.saturating_sub(1);
                    if let Some(failure) = done.failure {
                        store.failure = Some(failure.into());
                        // What was shown was a hope. The file knows better.
                        store.files = config::Snapshot::read();
                    }
                    cx.notify();
                });
                if landed.is_err() {
                    break;
                }
                if let Some(bar) = done.bar {
                    let _ = feeds.settings.update(cx, |feed, cx| {
                        if feed.value != bar {
                            feed.value = bar;
                            cx.notify();
                        }
                    });
                }
            }
        })
        .detach();

        cx.spawn(async move |store, cx| {
            let knobs = cx.background_spawn(async { hyprmod::knobs() }).await;
            let _ = store.update(cx, |store, cx| {
                (store.knobs, store.knobs_asked) = (knobs, true);
                cx.notify();
            });
        })
        .detach();

        cx.spawn(async move |store, cx| {
            let mut seen = logo::stamp();
            loop {
                cx.background_executor().timer(LOOK).await;
                let now = logo::stamp();
                if now == seen {
                    continue;
                }
                seen = now;
                let files = cx.background_spawn(async { config::Snapshot::read() }).await;
                let looked = store.update(cx, |store, cx| {
                    // Not while a change of our own is on its way to the
                    // file: what is on disk is older than what is shown.
                    if store.waiting == 0 && store.files != files {
                        store.files = files;
                        cx.notify();
                    }
                });
                if looked.is_err() {
                    break;
                }
            }
        })
        .detach();

        Store { files: config::Snapshot::read(), knobs: Value::Null, knobs_asked: false, failure: None, jobs, waiting: 0 }
    }

    /// Whether the compositor's knobs can be shown: the helper has answered
    /// and had something to say.
    pub fn knows_compositor(&self) -> Option<bool> {
        self.knobs_asked.then(|| self.knobs.as_object().is_some_and(|knobs| !knobs.is_empty()))
    }

    pub fn get(&self, source: Source) -> Option<&Value> {
        match source {
            Source::File(key) => self.files.get(key),
            Source::Knob(name) => self.knobs.get(name),
        }
    }

    pub fn flag(&self, source: Source, otherwise: bool) -> bool {
        self.get(source).and_then(Value::as_bool).unwrap_or(otherwise)
    }

    pub fn number(&self, source: Source, otherwise: f64) -> f64 {
        self.get(source).and_then(Value::as_f64).unwrap_or(otherwise)
    }

    pub fn text(&self, source: Source, otherwise: &str) -> String {
        self.get(source).and_then(Value::as_str).unwrap_or(otherwise).to_string()
    }

    /// A list of strings, as the file has it: a command and its arguments, a
    /// list of applications.
    pub fn words(&self, source: Source) -> Option<Vec<String>> {
        let list = self.get(source)?.as_array()?;
        Some(list.iter().filter_map(Value::as_str).map(str::to_string).collect())
    }

    /// The terminal the desktop uses, as a command: what the shell's config
    /// names, or what the compositor's keybind opens, or the one every
    /// machine with these dots has.
    pub fn terminal(&self) -> Vec<String> {
        let configured = self.words(shell("general.apps.terminal")).filter(|command| !command.is_empty());
        let bound = self.get(knob("terminal")).and_then(Value::as_str).filter(|name| !name.is_empty());
        configured.or_else(|| bound.map(|name| vec![name.to_string()])).unwrap_or_else(|| vec!["foot".to_string()])
    }

    pub fn set(&mut self, source: Source, value: Value, cx: &mut Context<Self>) {
        if self.get(source) == Some(&value) {
            return;
        }
        self.failure = None;
        match source {
            Source::File(key) => {
                self.files.set(key, value.clone());
                self.queue(format!("{:?} {}", key.file, key.path), true, move || config::write(key, value));
            }
            Source::Knob(name) => {
                config::put(&mut self.knobs, name, value.clone());
                // The helper takes a bare word, not JSON: `kitty`, not `"kitty"`.
                let said = value.as_str().map_or_else(|| value.to_string(), str::to_string);
                self.queue(format!("knob {name}"), false, move || hyprmod::set(name, &said));
            }
        }
        cx.notify();
    }

    /// Something else to be done in its turn, after whatever is already
    /// waiting: a keybind added, a monitor moved.
    pub fn run(&mut self, key: impl Into<String>, work: impl FnOnce() -> Result<(), String> + Send + 'static) {
        self.queue(key.into(), false, work);
    }

    fn queue(&mut self, key: String, rereads_bar: bool, work: impl FnOnce() -> Result<(), String> + Send + 'static) {
        self.waiting += 1;
        if self.jobs.send(Job { key, work: Box::new(work), rereads_bar }).is_err() {
            self.waiting -= 1;
            self.failure = Some("The settings could not be saved: nothing is writing them any more".into());
        }
    }
}

/// Does the jobs one at a time, in the order they were asked for, until the
/// window that asks for them has gone.
fn work_through(queued: mpsc::Receiver<Job>, finished: UnboundedSender<Done>) {
    while let Ok(first) = queued.recv() {
        let mut batch = vec![first];
        batch.extend(queued.try_iter());

        // Whatever piled up behind the first is looked at together, so that
        // a job another of the same key follows is skipped rather than done.
        let superseded: Vec<bool> = (0..batch.len())
            .map(|index| batch[index + 1..].iter().any(|later| later.key == batch[index].key))
            .collect();

        for (job, superseded) in batch.into_iter().zip(superseded) {
            let failure = if superseded { None } else { (job.work)().err() };
            let bar = job.rereads_bar.then(feeds::Settings::read);
            if finished.unbounded_send(Done { failure, bar }).is_err() {
                return;
            }
        }
    }
}

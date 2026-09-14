//! The decision engine: what happens when something suspicious execs, and the
//! bookkeeping for processes frozen while the user is asked about them.
//!
//! The posture is fail-open throughout, and deliberately so. A frozen process
//! the user never gets asked about is a hang with no explanation — the machine
//! looks broken and the cause is invisible. So a process is released when the
//! daemon is off, when no UI is connected to answer, when the detection no
//! longer holds on a second look, and when a prompt goes unanswered.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use redcommon::json::{self, Json};
use redcommon::procfs;
use redcommon::rules::{Rules, State, Verdict};
use redcommon::ui_sock::Clients;
use redcommon::{info, warn};

use crate::detect::{self, Detection};
use crate::signals;

/// A frozen process may never outlive this. The user may have missed the
/// prompt, or the shell may have gone down with it on screen.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(60);

/// What this daemon remembers about an executable. redwall's vocabulary is its
/// own (allow/deny), which is why the shared store is generic over this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Allow,
    Block,
}

impl Policy {
    /// A word from the rules tab. An unrecognised one blocks, which is the
    /// prompt's own default and the side a mistake can be undone from.
    pub fn from_ui(word: &str) -> Policy {
        Policy::parse(word).unwrap_or(Policy::Block)
    }
}

impl Verdict for Policy {
    fn as_str(self) -> &'static str {
        match self {
            Policy::Allow => "allow",
            Policy::Block => "block",
        }
    }

    fn parse(word: &str) -> Option<Policy> {
        match word {
            "allow" => Some(Policy::Allow),
            "block" => Some(Policy::Block),
            _ => None,
        }
    }
}

/// What the user answered. "Once" releases the process without remembering
/// anything, so it is not the same thing as a stored [`Policy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Allow,
    Block,
    Once,
}

impl Answer {
    /// A word off the socket. Anything unrecognised releases without
    /// remembering: this daemon fails open, and a UI that sends a word this
    /// version does not know must not kill the user's process over it.
    pub fn from_ui(word: &str) -> Answer {
        match Policy::parse(word) {
            Some(Policy::Allow) => Answer::Allow,
            Some(Policy::Block) => Answer::Block,
            None => Answer::Once,
        }
    }

    fn kills(self) -> bool {
        self == Answer::Block
    }

    fn worth_remembering(self) -> Option<Policy> {
        match self {
            Answer::Allow => Some(Policy::Allow),
            Answer::Block => Some(Policy::Block),
            Answer::Once => None,
        }
    }
}

struct Frozen {
    detection: Detection,
    asked_at: Instant,
    /// Wall-clock, for the UI. `asked_at` drives the timeout instead, because
    /// a clock that steps backwards must not strand a frozen process.
    ts: u64,
}

pub struct Guard {
    rules: Mutex<Rules<Policy>>,
    state: Mutex<State>,
    pending: Mutex<HashMap<u64, Frozen>>,
    next_ask_id: AtomicU64,
    /// Shared with the socket server, which adds and drops UIs as they come
    /// and go while this side only ever broadcasts.
    pub clients: Arc<Clients>,
    /// Freezing ourselves would stop the only process that could unfreeze us.
    self_pid: u32,
    simulate: bool,
}

impl Guard {
    pub fn new(rules: Rules<Policy>, state: State, simulate: bool) -> Guard {
        Guard {
            rules: Mutex::new(rules),
            state: Mutex::new(state),
            pending: Mutex::new(HashMap::new()),
            next_ask_id: AtomicU64::new(1),
            clients: Arc::new(Clients::new()),
            self_pid: std::process::id(),
            simulate,
        }
    }

    pub fn enabled(&self) -> bool {
        self.state.lock().unwrap().enabled
    }

    /// A process has just exec'd. Called for every exec on the machine, so the
    /// cheap tests come first and the overwhelming majority of programs leave
    /// through one of them without ever being signalled.
    pub fn on_exec(&self, pid: u32) {
        if !self.enabled() || pid == self.self_pid || pid <= 1 {
            return;
        }

        let comm = procfs::comm(pid).unwrap_or_default();
        let exe = procfs::exe(pid);
        if !detect::worth_inspecting(&comm, exe.as_ref()) {
            return;
        }

        // A remembered verdict answers before anything is frozen.
        if let Some(path) = exe.as_ref().map(|e| e.path.clone()) {
            match self.rules.lock().unwrap().action_for(&path) {
                Some(Policy::Allow) => return,
                Some(Policy::Block) => {
                    signals::terminate(pid);
                    let name = if comm.is_empty() { path.clone() } else { comm };
                    self.clients.broadcast(&json::obj([
                        ("t", json::s("event")),
                        ("kind", json::s("blocked")),
                        ("name", json::s(name)),
                        ("exe", json::s(path.clone())),
                        ("detail", json::s(format!("auto-blocked {path}"))),
                    ]));
                    info!("auto-blocked {path} (pid {pid})");
                    return;
                }
                None => {}
            }
        }

        // Classify BEFORE freezing. SIGSTOP on a foreground job makes the
        // user's shell reclaim the terminal ("suspended (signal)") even when
        // SIGCONT follows a millisecond later, and the interpreter pre-filter
        // covers every wrapper script in /usr/bin. Benign execs — nearly all
        // of them — must never be signalled at all. A real payload runs a few
        // extra milliseconds, which the freeze that follows still contains.
        if detect::judge(&detect::inspect(pid)).is_none() {
            return;
        }

        if !signals::freeze(pid) {
            return; // it exited while we were looking at it
        }

        // It ran unfrozen for those milliseconds and may have exec'd into
        // something else, so the detection that justifies a prompt is the one
        // taken after the freeze.
        let Some(detection) = detect::judge(&detect::inspect(pid)) else {
            signals::release(pid);
            return;
        };

        if !self.clients.any_connected() {
            signals::release(pid);
            self.broadcast_detection_event(&detection, "unmonitored");
            info!(
                "released with no UI to ask: {} ({}) — {}",
                detection.name, detection.exe, detection.detail
            );
            return;
        }

        self.ask(detection);
    }

    /// Freeze accounted for, prompt the user.
    fn ask(&self, detection: Detection) {
        let ask_id = self.next_ask_id.fetch_add(1, Ordering::Relaxed);
        let ts = now_secs();
        let msg = ask_message(ask_id, ts, &detection);
        self.pending.lock().unwrap().insert(
            ask_id,
            Frozen {
                detection,
                asked_at: Instant::now(),
                ts,
            },
        );
        self.clients.broadcast(&msg);
    }

    pub fn apply_verdict(&self, ask_id: u64, answer: Answer, remember: bool) {
        let Some(frozen) = self.pending.lock().unwrap().remove(&ask_id) else {
            return; // already resolved, or never ours
        };

        if answer.kills() {
            signals::terminate(frozen.detection.pid);
        } else {
            signals::release(frozen.detection.pid);
        }

        if remember {
            if let Some(policy) = answer.worth_remembering() {
                self.remember(&frozen.detection, policy);
            }
        }

        self.clients.broadcast(&json::obj([
            ("t", json::s("resolved")),
            ("id", json::n(ask_id as f64)),
        ]));
    }

    fn remember(&self, detection: &Detection, policy: Policy) {
        // "(pid 123)" is what an unreadable executable is reported as, and it
        // identifies nothing that could exec again.
        if !detection.exe.starts_with('/') {
            return;
        }
        if !self.store_rule(&detection.exe, policy, Some(&detection.name)) {
            return;
        }
        self.push_rules();
    }

    /// Write a rule, unless the executable is one that must never carry one.
    /// Returns whether it was stored.
    fn store_rule(&self, exe: &str, policy: Policy, name: Option<&str>) -> bool {
        if is_poison(exe) {
            warn!("refusing to remember a rule for the interpreter {exe}");
            return false;
        }
        self.rules.lock().unwrap().set(exe, policy, name);
        true
    }

    /// A rule edited from the rules tab rather than answered from a prompt.
    pub fn set_rule(&self, exe: &str, policy: Policy, name: Option<&str>) {
        if self.store_rule(exe, policy, name) {
            self.push_rules();
        }
    }

    pub fn delete_rule(&self, exe: &str) {
        self.rules.lock().unwrap().delete(exe);
        self.push_rules();
    }

    pub fn push_rules(&self) {
        let snapshot = self.rules_snapshot();
        self.clients
            .broadcast(&json::obj([("t", json::s("rules")), ("rules", snapshot)]));
    }

    pub fn rules_snapshot(&self) -> Json {
        self.rules.lock().unwrap().snapshot()
    }

    /// Drop any stored rule this daemon must not act on. Rule files outlive
    /// the code that wrote them, and an interpreter rule is poison in both
    /// directions: a remembered block on /usr/bin/bash kills every shell
    /// script on the machine forever, and a remembered allow switches the
    /// reverse-shell detection off wholesale.
    pub fn purge_poison_rules(&self) {
        for exe in self.rules.lock().unwrap().purge(is_poison) {
            info!("dropped unsafe interpreter rule: {exe}");
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.lock().unwrap().set(enabled);
        if !enabled {
            // Nothing may stay frozen by a daemon that is no longer watching.
            self.release_all("protection disabled");
        }
        self.clients.broadcast(&json::obj([
            ("t", json::s("state")),
            ("enabled", Json::Bool(enabled)),
        ]));
    }

    /// The prompts a freshly-connected UI has missed.
    pub fn waiting_asks(&self) -> Vec<Json> {
        self.pending
            .lock()
            .unwrap()
            .iter()
            .map(|(&id, f)| ask_message(id, f.ts, &f.detection))
            .collect()
    }

    /// Release everything frozen right now and withdraw its prompts.
    pub fn release_all(&self, reason: &str) {
        let held: Vec<(u64, u32)> = {
            let mut pending = self.pending.lock().unwrap();
            pending
                .drain()
                .map(|(id, f)| (id, f.detection.pid))
                .collect()
        };
        for &(id, pid) in &held {
            signals::release(pid);
            self.clients.broadcast(&json::obj([
                ("t", json::s("resolved")),
                ("id", json::n(id as f64)),
            ]));
        }
        if !held.is_empty() {
            info!("released {} frozen process(es): {reason}", held.len());
        }
    }

    /// Nothing may stay frozen forever waiting for a verdict that is not
    /// coming. A stopped process is invisible to the user who started it.
    pub fn reap_stale(&self) {
        let stale: Vec<(u64, Detection)> = {
            let mut pending = self.pending.lock().unwrap();
            let ids: Vec<u64> = pending
                .iter()
                .filter(|(_, f)| f.asked_at.elapsed() > ASK_TIMEOUT)
                .map(|(&id, _)| id)
                .collect();
            ids.into_iter()
                .filter_map(|id| pending.remove(&id).map(|f| (id, f.detection)))
                .collect()
        };

        for (id, detection) in stale {
            signals::release(detection.pid);
            self.clients.broadcast(&json::obj([
                ("t", json::s("resolved")),
                ("id", json::n(id as f64)),
            ]));
            self.clients.broadcast(&json::obj([
                ("t", json::s("event")),
                ("kind", json::s("timeout-release")),
                ("name", json::s(detection.name.clone())),
                ("exe", json::s(detection.exe.clone())),
                (
                    "detail",
                    json::s(format!(
                        "no verdict within {}s — released",
                        ASK_TIMEOUT.as_secs()
                    )),
                ),
            ]));
            info!("released after timeout: {}", detection.exe);
        }
    }

    /// A synthetic detection, for exercising the UI without a payload. Only
    /// reachable with --simulate, where no process is ever frozen, so the pid
    /// it carries is never signalled.
    pub fn inject(&self, msg: &Json) {
        if !self.simulate {
            return;
        }
        let kind = match msg.str_field("kind") {
            Some(detect::FOREIGN_EXEC) => detect::FOREIGN_EXEC,
            _ => detect::REVERSE_SHELL,
        };
        self.ask(Detection {
            pid: msg.get("pid").and_then(Json::as_u64).unwrap_or(0) as u32,
            exe: msg.str_field("exe").unwrap_or("/tmp/payload").to_string(),
            name: msg.str_field("name").unwrap_or("payload").to_string(),
            kind,
            parent: msg.str_field("parent").unwrap_or("firefox").to_string(),
            detail: msg
                .str_field("detail")
                .unwrap_or("simulated detection")
                .to_string(),
        });
    }

    fn broadcast_detection_event(&self, detection: &Detection, kind: &str) {
        self.clients.broadcast(&json::obj([
            ("t", json::s("event")),
            ("kind", json::s(kind)),
            ("pid", json::n(detection.pid)),
            ("exe", json::s(detection.exe.clone())),
            ("name", json::s(detection.name.clone())),
            ("parent", json::s(detection.parent.clone())),
            ("detail", json::s(detection.detail.clone())),
        ]));
    }
}

/// An executable no rule may ever name.
fn is_poison(exe: &str) -> bool {
    detect::is_interpreter(procfs::file_name(exe))
}

fn ask_message(ask_id: u64, ts: u64, d: &Detection) -> Json {
    json::obj([
        ("t", json::s("ask")),
        ("id", json::n(ask_id as f64)),
        ("pid", json::n(d.pid)),
        ("exe", json::s(d.exe.clone())),
        ("name", json::s(d.name.clone())),
        ("kind", json::s(d.kind)),
        ("parent", json::s(d.parent.clone())),
        ("detail", json::s(d.detail.clone())),
        ("ts", json::n(ts as f64)),
    ])
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};

    /// A real process running from a world-writable directory — the
    /// foreign-exec detection, produced without writing a payload: a copy of
    /// `sleep` in /tmp is exactly as suspicious as anything else there.
    struct FakePayload {
        child: Child,
        path: PathBuf,
    }

    impl FakePayload {
        fn spawn(tag: &str) -> FakePayload {
            let dir = std::env::temp_dir().join(format!("redguard-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(tag);
            // Copied by a child process rather than by us: a write handle held
            // in this process is inherited by every other test's spawn until
            // it execs, and the kernel refuses to run a file anyone still has
            // open for writing (ETXTBSY). `cp` has exited by the time this
            // returns, so nothing holds the payload open.
            let copied = Command::new("cp")
                .arg("/usr/bin/sleep")
                .arg(&path)
                .status()
                .expect("copy a binary into scratch space");
            assert!(copied.success(), "could not stage the payload");

            let child = Command::new(&path)
                .arg("30")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("run it from there");
            let payload = FakePayload { child, path };

            // A spawn returns as soon as the fork succeeds, so for a moment
            // the pid is still a copy of this test binary. The daemon only
            // ever sees a process after its exec, and so must the test.
            for _ in 0..200 {
                if procfs::exe(payload.pid()).is_some_and(|e| e.path == payload.path.to_string_lossy())
                {
                    return payload;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the payload never reached its exec");
        }

        fn pid(&self) -> u32 {
            self.child.id()
        }

        fn state(&self) -> Option<char> {
            let stat = std::fs::read_to_string(format!("/proc/{}/stat", self.pid())).ok()?;
            stat[stat.rfind(')')? + 1..]
                .split_ascii_whitespace()
                .next()?
                .chars()
                .next()
        }

        fn settles_at(&self, want: char) -> bool {
            for _ in 0..200 {
                if self.state() == Some(want) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        }
    }

    impl Drop for FakePayload {
        fn drop(&mut self) {
            signals::release(self.pid());
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn guard(tag: &str) -> Guard {
        let dir = std::env::temp_dir().join(format!("redguard-guard-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("rules.json"));
        let _ = std::fs::remove_file(dir.join("state.json"));
        Guard::new(
            Rules::load(dir.join("rules.json")),
            State::load(dir.join("state.json")),
            false,
        )
    }

    #[test]
    fn a_binary_in_scratch_space_is_frozen_and_asked_about() {
        let g = guard("ask");
        g.clients.set_test_connected(true);
        let payload = FakePayload::spawn("frozen");

        g.on_exec(payload.pid());
        assert!(payload.settles_at('T'), "frozen while the user is asked");

        let pending = g.pending.lock().unwrap();
        assert_eq!(pending.len(), 1);
        let (_, frozen) = pending.iter().next().unwrap();
        assert_eq!(frozen.detection.kind, detect::FOREIGN_EXEC);
        assert_eq!(frozen.detection.pid, payload.pid());
    }

    #[test]
    fn allowing_once_releases_without_remembering() {
        let g = guard("once");
        g.clients.set_test_connected(true);
        let payload = FakePayload::spawn("once");

        g.on_exec(payload.pid());
        assert!(payload.settles_at('T'));
        let ask_id = *g.pending.lock().unwrap().keys().next().unwrap();

        g.apply_verdict(ask_id, Answer::Once, false);
        assert!(payload.settles_at('S'), "running again");
        assert!(g.pending.lock().unwrap().is_empty());
        assert_eq!(g.rules_snapshot().dump(), "[]", "nothing remembered");
    }

    #[test]
    fn blocking_kills_the_process_and_remembers_it() {
        let g = guard("block");
        g.clients.set_test_connected(true);
        let mut payload = FakePayload::spawn("blocked");
        let exe = payload.path.to_string_lossy().into_owned();

        g.on_exec(payload.pid());
        let ask_id = *g.pending.lock().unwrap().keys().next().unwrap();
        g.apply_verdict(ask_id, Answer::Block, true);

        let status = payload.child.wait().expect("reaped");
        assert!(!status.success(), "killed");
        assert!(g.rules_snapshot().dump().contains(&exe), "remembered");
        assert_eq!(
            g.rules.lock().unwrap().action_for(&exe),
            Some(Policy::Block)
        );
    }

    #[test]
    fn a_remembered_block_kills_on_sight_without_a_prompt() {
        let g = guard("auto");
        g.clients.set_test_connected(true);
        let mut payload = FakePayload::spawn("auto");
        let exe = payload.path.to_string_lossy().into_owned();
        g.set_rule(&exe, Policy::Block, Some("auto"));

        g.on_exec(payload.pid());
        let status = payload.child.wait().expect("reaped");
        assert!(!status.success(), "killed on sight");
        assert!(g.pending.lock().unwrap().is_empty(), "no prompt for a decided app");
    }

    #[test]
    fn a_remembered_allow_is_never_frozen() {
        let g = guard("allowed");
        g.clients.set_test_connected(true);
        let payload = FakePayload::spawn("allowed");
        let exe = payload.path.to_string_lossy().into_owned();
        g.set_rule(&exe, Policy::Allow, Some("allowed"));

        g.on_exec(payload.pid());
        assert!(payload.settles_at('S'), "left running");
        assert!(g.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn nothing_is_frozen_with_no_ui_to_ask() {
        let g = guard("noui");
        g.clients.set_test_connected(false);
        let payload = FakePayload::spawn("noui");

        g.on_exec(payload.pid());
        assert!(payload.settles_at('S'), "released rather than left stopped");
        assert!(g.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn nothing_is_frozen_when_protection_is_off() {
        let g = guard("off");
        g.clients.set_test_connected(true);
        g.set_enabled(false);
        let payload = FakePayload::spawn("off");

        g.on_exec(payload.pid());
        assert!(payload.settles_at('S'));
        assert!(g.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn an_unanswered_prompt_releases_the_process() {
        let g = guard("timeout");
        g.clients.set_test_connected(true);
        let payload = FakePayload::spawn("timeout");

        g.on_exec(payload.pid());
        assert!(payload.settles_at('T'));
        {
            let mut pending = g.pending.lock().unwrap();
            let frozen = pending.values_mut().next().unwrap();
            frozen.asked_at = Instant::now() - ASK_TIMEOUT - Duration::from_secs(1);
        }

        g.reap_stale();
        assert!(payload.settles_at('S'), "let go rather than left frozen");
        assert!(g.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn disabling_releases_everything_frozen() {
        let g = guard("disable");
        g.clients.set_test_connected(true);
        let payload = FakePayload::spawn("disable");

        g.on_exec(payload.pid());
        assert!(payload.settles_at('T'));

        g.set_enabled(false);
        assert!(payload.settles_at('S'));
        assert!(g.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn interpreter_rules_are_refused_and_purged() {
        let g = guard("poison");
        g.set_rule("/usr/bin/bash", Policy::Block, Some("bash"));
        assert_eq!(g.rules_snapshot().dump(), "[]", "never written in the first place");

        // One that got into the file before this rule existed.
        g.rules
            .lock()
            .unwrap()
            .set("/usr/bin/python3", Policy::Allow, None);
        g.purge_poison_rules();
        assert_eq!(g.rules_snapshot().dump(), "[]", "and removed on load");
    }

    #[test]
    fn the_daemon_never_freezes_itself_or_init() {
        let g = guard("self");
        g.clients.set_test_connected(true);
        g.on_exec(std::process::id());
        g.on_exec(1);
        assert!(g.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn a_verdict_for_an_unknown_prompt_is_ignored() {
        let g = guard("unknown");
        g.apply_verdict(9999, Answer::Block, true);
        assert!(g.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn an_unknown_answer_releases_rather_than_kills() {
        assert_eq!(Answer::from_ui("allow"), Answer::Allow);
        assert_eq!(Answer::from_ui("block"), Answer::Block);
        assert_eq!(Answer::from_ui("once"), Answer::Once);
        assert_eq!(Answer::from_ui("something-new"), Answer::Once);
        assert_eq!(Answer::from_ui("once").worth_remembering(), None);
        assert!(!Answer::from_ui("once").kills());
    }

    #[test]
    fn a_replayed_ask_carries_everything_the_prompt_renders() {
        let g = guard("replay");
        g.clients.set_test_connected(true);
        g.ask(Detection {
            pid: 42,
            exe: "/tmp/x/beacon".into(),
            name: "beacon".into(),
            kind: detect::FOREIGN_EXEC,
            parent: "firefox".into(),
            detail: "executable lives in a world-writable scratch dir".into(),
        });

        let asks = g.waiting_asks();
        assert_eq!(asks.len(), 1);
        let dump = asks[0].dump();
        for field in ["\"id\"", "\"pid\"", "\"exe\"", "\"name\"", "\"kind\"", "\"parent\"", "\"detail\"", "beacon", "firefox"] {
            assert!(dump.contains(field), "{field} missing from {dump}");
        }
    }
}

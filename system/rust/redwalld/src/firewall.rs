//! The decision engine: what happens to a new outbound connection, and the
//! bookkeeping for packets held while the user is asked about them.
//!
//! Every fail-open property of the Python daemon is preserved deliberately,
//! because the failure mode on the other side is a machine with no working
//! network. Packets are accepted when the firewall is disabled, when no UI is
//! connected to answer, when one app has already piled up too many held
//! packets, and when a prompt goes unanswered past the timeout.

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use redcommon::json::{self, Json};
use redcommon::rules::{Rules, State, Verdict};
use redcommon::ui_sock::Clients;
use redcommon::{info, warn};

use crate::netlink::{Nfqueue, NF_ACCEPT, NF_DROP};
use crate::packet::Conn;
use crate::proc::{Attribution, Attributor};

/// What this daemon remembers about an executable. redguard's vocabulary is
/// its own (allow/block), which is why the shared store is generic over this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Allow,
    Deny,
}

impl Verdict for Action {
    fn as_str(self) -> &'static str {
        match self {
            Action::Allow => "allow",
            Action::Deny => "deny",
        }
    }

    fn parse(word: &str) -> Option<Action> {
        match word {
            "allow" => Some(Action::Allow),
            "deny" => Some(Action::Deny),
            _ => None,
        }
    }
}

impl Action {
    /// A verdict arriving from the UI. Anything that is not a clear "allow"
    /// denies: the socket is trusted, but a typo must not open the network.
    pub fn from_ui(word: &str) -> Action {
        match Action::parse(word) {
            Some(a) => a,
            None => Action::Deny,
        }
    }
}

/// A held connection may never outlive this. nftables' `bypass` only fails
/// open when *nothing* is bound to the queue, so a daemon that is bound but
/// waiting on a prompt nobody answers has to bound the wait itself.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(60);

/// One unanswered app must not fill the queue on its own: a held SYN is
/// retransmitted at 1s/3s/7s/15s/31s and every parallel connection lands here
/// too. Past the cap the extras go through; the prompt still decides the
/// app's future.
pub const MAX_HELD_PER_EXE: usize = 32;

struct Pending {
    ask_id: u64,
    packet_ids: Vec<u32>,
    asked_at: Instant,
    name: String,
    dst: String,
    port: u16,
    proto: String,
}

pub struct Firewall {
    queue: Option<Nfqueue>,
    /// Simulation only stages connections that never existed, so it is gated
    /// on the flag the binary was started with rather than on the absence of
    /// a queue.
    simulate: bool,
    rules: Mutex<Rules<Action>>,
    state: Mutex<State>,
    pending: Mutex<HashMap<String, Pending>>,
    attributor: Mutex<Attributor>,
    next_ask_id: AtomicU64,
    /// Shared with the socket server, which adds and drops UIs as they come
    /// and go while this side only ever broadcasts.
    pub clients: Arc<Clients>,
}

impl Firewall {
    pub fn new(
        queue: Option<Nfqueue>,
        rules: Rules<Action>,
        state: State,
        simulate: bool,
    ) -> Firewall {
        Firewall {
            queue,
            simulate,
            rules: Mutex::new(rules),
            state: Mutex::new(state),
            pending: Mutex::new(HashMap::new()),
            attributor: Mutex::new(Attributor::new()),
            next_ask_id: AtomicU64::new(1),
            clients: Arc::new(Clients::new()),
        }
    }

    pub fn attribute(&self, proto: &str, sport: u16) -> Option<Attribution> {
        self.attributor.lock().unwrap().attribute(proto, sport)
    }

    /// Block until the kernel queues packets. Simulate mode has no queue, so
    /// the packet thread simply parks.
    pub fn recv_packets(&self, buf: &mut [u8]) -> std::io::Result<Vec<crate::netlink::Queued>> {
        match &self.queue {
            Some(q) => q.recv(buf),
            None => {
                std::thread::sleep(Duration::from_secs(3600));
                Ok(Vec::new())
            }
        }
    }

    /// Let a packet through without consulting the rules — used for anything
    /// we cannot parse or attribute, because an unknown is not a decision.
    pub fn accept(&self, packet_id: u32) {
        self.verdict(packet_id, NF_ACCEPT);
    }

    fn verdict(&self, packet_id: u32, verdict: u32) {
        if let Some(q) = &self.queue {
            if let Err(e) = q.verdict(packet_id, verdict) {
                warn!("verdict for {packet_id} failed: {e}");
            }
        }
    }

    fn verdict_all(&self, ids: &[u32], verdict: u32) {
        for &id in ids {
            self.verdict(id, verdict);
        }
    }

    pub fn enabled(&self) -> bool {
        self.state.lock().unwrap().enabled
    }

    /// Decide a freshly queued packet. Called on the packet thread.
    pub fn handle(&self, packet_id: u32, conn: &Conn, who: &Attribution) {
        if !self.enabled() {
            return self.verdict(packet_id, NF_ACCEPT); // rules retained, traffic passes
        }

        match self.rules.lock().unwrap().action_for(&who.exe) {
            Some(Action::Allow) => return self.verdict(packet_id, NF_ACCEPT),
            Some(Action::Deny) => return self.verdict(packet_id, NF_DROP),
            None => {}
        }

        // Never prompt for DNS: a held lookup stalls name resolution for the
        // whole machine. A denied app was already dropped above, so this only
        // lets allowed and not-yet-decided apps resolve.
        if conn.dport == 53 {
            return self.verdict(packet_id, NF_ACCEPT);
        }

        if !self.clients.any_connected() {
            return self.verdict(packet_id, NF_ACCEPT); // nobody to ask
        }

        let ask = {
            let mut pending = self.pending.lock().unwrap();
            match pending.get_mut(&who.exe) {
                Some(entry) => {
                    // One app, one prompt. Retransmits and parallel connections
                    // from it must not starve the queue for everything else.
                    if entry.packet_ids.len() >= MAX_HELD_PER_EXE {
                        drop(pending);
                        return self.verdict(packet_id, NF_ACCEPT);
                    }
                    entry.packet_ids.push(packet_id);
                    None
                }
                None => {
                    let ask_id = self.next_ask_id.fetch_add(1, Ordering::Relaxed);
                    pending.insert(
                        who.exe.clone(),
                        Pending {
                            ask_id,
                            packet_ids: vec![packet_id],
                            asked_at: Instant::now(),
                            name: who.name.clone(),
                            dst: conn.daddr.clone(),
                            port: conn.dport,
                            proto: conn.proto.to_string(),
                        },
                    );
                    Some(ask_id)
                }
            }
        };

        if let Some(ask_id) = ask {
            self.clients.broadcast(&json::obj([
                ("t", json::s("ask")),
                ("id", json::n(ask_id as f64)),
                ("exe", json::s(who.exe.clone())),
                ("name", json::s(who.name.clone())),
                ("dst", json::s(conn.daddr.clone())),
                ("port", json::n(conn.dport)),
                ("proto", json::s(conn.proto)),
                ("pid", json::n(who.pid)),
            ]));
        }
    }

    /// Release every packet held for `exe` with one verdict, and forget it.
    fn resolve(&self, exe: &str, allow: bool) {
        let entry = self.pending.lock().unwrap().remove(exe);
        if let Some(p) = entry {
            self.verdict_all(&p.packet_ids, if allow { NF_ACCEPT } else { NF_DROP });
        }
    }

    pub fn apply_verdict(&self, ask_id: u64, action: Action, remember: bool) {
        let exe = self
            .pending
            .lock()
            .unwrap()
            .iter()
            .find(|(_, p)| p.ask_id == ask_id)
            .map(|(exe, _)| exe.clone());
        let Some(exe) = exe else { return };

        let name = self
            .pending
            .lock()
            .unwrap()
            .get(&exe)
            .map(|p| p.name.clone());

        if remember {
            self.rules
                .lock()
                .unwrap()
                .set(&exe, action, name.as_deref());
            self.push_rules();
        }
        self.resolve(&exe, action == Action::Allow);
        self.clients
            .broadcast(&json::obj([("t", json::s("resolved")), ("exe", json::s(exe.clone()))]));

        if action == Action::Deny {
            self.cut_connections(&exe);
        }
    }

    pub fn set_rule(&self, exe: &str, action: Action, name: Option<&str>) {
        self.rules.lock().unwrap().set(exe, action, name);

        let waiting = self.pending.lock().unwrap().contains_key(exe);
        if waiting {
            self.resolve(exe, action == Action::Allow);
            self.clients
                .broadcast(&json::obj([("t", json::s("resolved")), ("exe", json::s(exe))]));
        }
        if action == Action::Deny {
            self.cut_connections(exe);
        }
        self.push_rules();
    }

    pub fn delete_rule(&self, exe: &str) {
        self.rules.lock().unwrap().delete(exe);
        self.push_rules();
    }

    pub fn push_rules(&self) {
        let snapshot = self.rules.lock().unwrap().snapshot();
        self.clients
            .broadcast(&json::obj([("t", json::s("rules")), ("rules", snapshot)]));
    }

    pub fn rules_snapshot(&self) -> Json {
        self.rules.lock().unwrap().snapshot()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.lock().unwrap().set(enabled);
        if !enabled {
            // Passing everything means nothing may stay held; release the
            // packets and dismiss the popups rather than stranding them.
            self.release_all("firewall disabled");
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
            .map(|(exe, p)| {
                json::obj([
                    ("t", json::s("ask")),
                    ("id", json::n(p.ask_id as f64)),
                    ("exe", json::s(exe.clone())),
                    ("name", json::s(p.name.clone())),
                    ("dst", json::s(p.dst.clone())),
                    ("port", json::n(p.port)),
                    ("proto", json::s(p.proto.clone())),
                ])
            })
            .collect()
    }

    /// Accept everything held right now and withdraw its prompts.
    pub fn release_all(&self, reason: &str) {
        let held: Vec<String> = self.pending.lock().unwrap().keys().cloned().collect();
        for exe in &held {
            self.resolve(exe, true);
            self.clients
                .broadcast(&json::obj([("t", json::s("resolved")), ("exe", json::s(exe.clone()))]));
        }
        if !held.is_empty() {
            info!("released {} held app(s): {reason}", held.len());
        }
    }

    /// Nothing may stay held forever waiting for a verdict: once the kernel
    /// queue fills, every new outbound connection on the machine is dropped
    /// and the box looks frozen even though the desktop is fine.
    pub fn reap_stale(&self) {
        let stale: Vec<String> = self
            .pending
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, p)| p.asked_at.elapsed() > ASK_TIMEOUT)
            .map(|(exe, _)| exe.clone())
            .collect();
        for exe in &stale {
            self.resolve(exe, true);
            self.clients
                .broadcast(&json::obj([("t", json::s("resolved")), ("exe", json::s(exe.clone()))]));
            info!("prompt for {exe} timed out, letting it through");
        }
    }

    /// A connection that never happened, for exercising the prompt and the
    /// rule engine without root or a queue. Only reachable with --simulate,
    /// where there is no packet to verdict and nothing is ever held for real.
    pub fn inject(&self, msg: &Json) {
        if !self.simulate {
            return;
        }
        let exe = msg
            .str_field("exe")
            .unwrap_or("/usr/bin/unknownapp")
            .to_string();
        let conn = Conn {
            proto: if msg.str_field("proto") == Some("udp") { "udp" } else { "tcp" },
            saddr: "0.0.0.0".to_string(),
            sport: 0,
            daddr: msg.str_field("dst").unwrap_or("203.0.113.7").to_string(),
            dport: msg.get("port").and_then(Json::as_u64).unwrap_or(443) as u16,
        };
        let who = Attribution {
            name: msg
                .str_field("name")
                .unwrap_or_else(|| redcommon::procfs::file_name(&exe))
                .to_string(),
            exe,
            pid: msg.get("pid").and_then(Json::as_u64).unwrap_or(0) as u32,
        };
        // No packet id exists for a connection nobody sent; with no queue
        // bound, every verdict on it is a no-op anyway.
        self.handle(0, &conn, &who);
    }

    /// Denying an app should stop it now, not at its next connection. Its
    /// established flows live in conntrack, which the OUTPUT hook no longer
    /// sees, so they have to be torn down explicitly.
    fn cut_connections(&self, exe: &str) {
        let ports = self.attributor.lock().unwrap().ports_for_exe(exe);
        for (proto, port) in ports {
            let _ = Command::new("conntrack")
                .args(["-D", "-p", &proto, "--orig-port-src", &port.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::Conn;

    /// One firewall with its own rule and state files. Tests run in parallel
    /// threads, so sharing a rules.json between them lets one test's verdict
    /// answer another test's prompt.
    fn fixture(tag: &str) -> (Firewall, Conn, Attribution) {
        let dir = std::env::temp_dir()
            .join(format!("redwall-fw-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("rules.json"));
        let _ = std::fs::remove_file(dir.join("state.json"));
        let fw = Firewall::new(
            None, // no queue: verdicts become no-ops, the bookkeeping still runs
            Rules::load(dir.join("rules.json")),
            State::load(dir.join("state.json")),
            false,
        );
        let conn = Conn {
            proto: "tcp",
            saddr: "10.0.0.2".into(),
            sport: 40000,
            daddr: "1.1.1.1".into(),
            dport: 443,
        };
        let who = Attribution {
            exe: "/usr/bin/curl".into(),
            name: "curl".into(),
            pid: 1234,
        };
        (fw, conn, who)
    }

    #[test]
    fn one_app_gets_one_prompt_however_many_packets() {
        let (fw, conn, who) = fixture("one_app_gets_one_prompt_however_many_packets");
        fw.clients.set_test_connected(true);
        for id in 0..5 {
            fw.handle(id, &conn, &who);
        }
        let pending = fw.pending.lock().unwrap();
        assert_eq!(pending.len(), 1, "deduped to a single prompt");
        assert_eq!(pending["/usr/bin/curl"].packet_ids.len(), 5);
    }

    #[test]
    fn a_single_app_cannot_fill_the_queue() {
        let (fw, conn, who) = fixture("a_single_app_cannot_fill_the_queue");
        fw.clients.set_test_connected(true);
        for id in 0..(MAX_HELD_PER_EXE as u32 + 20) {
            fw.handle(id, &conn, &who);
        }
        assert_eq!(
            fw.pending.lock().unwrap()["/usr/bin/curl"].packet_ids.len(),
            MAX_HELD_PER_EXE,
            "extras are let through rather than held"
        );
    }

    #[test]
    fn fails_open_with_no_ui_and_when_disabled() {
        let (fw, conn, who) = fixture("fails_open_with_no_ui_and_when_disabled");

        fw.clients.set_test_connected(false);
        fw.handle(1, &conn, &who);
        assert!(fw.pending.lock().unwrap().is_empty(), "nothing held with no UI");

        fw.clients.set_test_connected(true);
        fw.set_enabled(false);
        fw.handle(2, &conn, &who);
        assert!(fw.pending.lock().unwrap().is_empty(), "nothing held when disabled");
    }

    #[test]
    fn dns_is_never_held() {
        let (fw, mut conn, who) = fixture("dns_is_never_held");
        fw.clients.set_test_connected(true);
        conn.dport = 53;
        fw.handle(1, &conn, &who);
        assert!(fw.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn a_remembered_verdict_answers_without_prompting() {
        let (fw, conn, who) = fixture("a_remembered_verdict_answers_without_prompting");
        fw.clients.set_test_connected(true);
        fw.set_rule(&who.exe, Action::Allow, Some("curl"));
        fw.handle(1, &conn, &who);
        assert!(fw.pending.lock().unwrap().is_empty(), "allowed app is not asked about");

        fw.set_rule(&who.exe, Action::Deny, Some("curl"));
        fw.handle(2, &conn, &who);
        assert!(fw.pending.lock().unwrap().is_empty(), "denied app is not asked about");
    }

    #[test]
    fn answering_clears_the_hold_and_remembers() {
        let (fw, conn, who) = fixture("answering_clears_the_hold_and_remembers");
        fw.clients.set_test_connected(true);
        fw.handle(1, &conn, &who);
        let ask_id = fw.pending.lock().unwrap()["/usr/bin/curl"].ask_id;

        fw.apply_verdict(ask_id, Action::Allow, true);
        assert!(fw.pending.lock().unwrap().is_empty());
        assert_eq!(
            fw.rules.lock().unwrap().action_for("/usr/bin/curl"),
            Some(Action::Allow)
        );
    }

    #[test]
    fn a_staged_connection_prompts_only_when_simulating() {
        let dir = std::env::temp_dir().join(format!("redwall-fw-{}-sim", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("rules.json"));
        let _ = std::fs::remove_file(dir.join("state.json"));
        let staged = json::obj([
            ("exe", json::s("/opt/telemetry/tracker")),
            ("name", json::s("tracker")),
            ("dst", json::s("8.8.8.8")),
            ("port", json::n(4444u32)),
            ("proto", json::s("udp")),
        ]);

        let real = Firewall::new(
            None,
            Rules::load(dir.join("rules.json")),
            State::load(dir.join("state.json")),
            false,
        );
        real.clients.set_test_connected(true);
        real.inject(&staged);
        assert!(
            real.pending.lock().unwrap().is_empty(),
            "a live daemon invents no connections"
        );

        let sim = Firewall::new(
            None,
            Rules::load(dir.join("rules.json")),
            State::load(dir.join("state.json")),
            true,
        );
        sim.clients.set_test_connected(true);
        sim.inject(&staged);
        let pending = sim.pending.lock().unwrap();
        let held = &pending["/opt/telemetry/tracker"];
        assert_eq!(held.name, "tracker");
        assert_eq!(held.port, 4444);
        assert_eq!(held.proto, "udp");
    }

    #[test]
    fn an_unanswered_prompt_is_released_not_left_holding() {
        let (fw, conn, who) = fixture("an_unanswered_prompt_is_released_not_left_holding");
        fw.clients.set_test_connected(true);
        fw.handle(1, &conn, &who);
        fw.pending
            .lock()
            .unwrap()
            .get_mut("/usr/bin/curl")
            .unwrap()
            .asked_at = Instant::now() - ASK_TIMEOUT - Duration::from_secs(1);

        fw.reap_stale();
        assert!(fw.pending.lock().unwrap().is_empty(), "timed-out hold is let through");
    }
}

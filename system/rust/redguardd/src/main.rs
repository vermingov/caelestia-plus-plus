//! redguardd — behavioural process protection.
//!
//! A small host intrusion detector for this desktop. It watches process execs
//! in real time through the kernel's proc connector and, on a high-confidence
//! detection, freezes the process and asks the user through the bar: allow,
//! block, or allow once. The answer is remembered per-executable and
//! persisted, so it survives reboots.
//!
//! It is the companion to redwall: redwall governs who may talk to the
//! network, redguard governs whether a process is behaving like an exploit
//! payload. It touches no networking at all, so it is VPN-agnostic.
//!
//! This is a Rust rewrite of the Python daemon it replaces, drop-in on both
//! interfaces that matter: the same newline-JSON protocol on the same Unix
//! socket, and the same `rules.json` / `state.json` on disk.
//!
//! Honest about what it is: best effort, not a kernel-enforced sandbox. There
//! is a small window between exec and freeze, and for the interactive payloads
//! this targets — a reverse shell waiting on its operator, a dropper about to
//! act — the freeze lands inside it. Protection is only enforced while the
//! shell is running, because the shell is what asks the question.
//!
//! Run with --simulate to exercise the rule engine and the UI protocol with
//! synthetic detections: no root, no netlink, and nothing is ever frozen.

mod detect;
mod guard;
mod procconn;
mod signals;
mod ui;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use redcommon::rules::{Rules, State};
use redcommon::{info, ui_sock, warn};

use guard::Guard;
use procconn::ProcConnector;

const DEFAULT_SOCK: &str = "/run/redguard/ui.sock";
const DEFAULT_RULES: &str = "/var/lib/redguard/rules.json";

/// How often frozen processes are checked against the prompt timeout.
const REAP_INTERVAL: Duration = Duration::from_secs(5);

struct Args {
    sock: String,
    rules: String,
    ui_gid: u32,
    simulate: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        sock: DEFAULT_SOCK.to_string(),
        rules: DEFAULT_RULES.to_string(),
        ui_gid: 1000,
        simulate: false,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--simulate" => args.simulate = true,
            "--sock" => {
                i += 1;
                if let Some(v) = argv.get(i) {
                    args.sock = v.clone();
                }
            }
            "--rules" => {
                i += 1;
                if let Some(v) = argv.get(i) {
                    args.rules = v.clone();
                }
            }
            "--ui-gid" => {
                i += 1;
                if let Some(v) = argv.get(i).and_then(|v| v.parse().ok()) {
                    args.ui_gid = v;
                }
            }
            "-h" | "--help" => {
                println!(
                    "redguardd [--sock PATH] [--rules PATH] [--ui-gid GID] [--simulate]\n\
                     \n\
                     --simulate  run the UI protocol with synthetic detections,\n\
                     \x20            without the proc connector or root"
                );
                std::process::exit(0);
            }
            other => warn!("ignoring unknown argument {other}"),
        }
        i += 1;
    }
    args
}

fn main() {
    redcommon::log::set_tag("redguard");
    let args = parse_args();

    let state_path = std::path::Path::new(&args.rules)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("state.json");

    let guard = Arc::new(Guard::new(
        Rules::load(&args.rules),
        State::load(state_path),
        args.simulate,
    ));
    guard.purge_poison_rules();

    if !args.simulate {
        let watcher = Arc::clone(&guard);
        thread::Builder::new()
            .name("proc-connector".into())
            .spawn(move || watch_execs(watcher))
            .expect("cannot start the exec monitor");
    }

    let reaper = Arc::clone(&guard);
    thread::Builder::new()
        .name("reaper".into())
        .spawn(move || loop {
            thread::sleep(REAP_INTERVAL);
            reaper.reap_stale();
        })
        .expect("cannot start the timeout reaper");

    if args.simulate {
        let seeder = Arc::clone(&guard);
        thread::Builder::new()
            .name("simulate".into())
            .spawn(move || seed_simulated_detections(seeder))
            .expect("cannot start the simulation");
    }

    info!(
        "listening on {} (rules {}){}",
        args.sock,
        args.rules,
        if args.simulate { ", simulate mode" } else { "" }
    );

    let clients = Arc::clone(&guard.clients);
    if let Err(e) = ui_sock::serve(Arc::new(ui::Ui::new(guard)), clients, &args.sock, args.ui_gid) {
        warn!("UI socket failed: {e}");
        std::process::exit(1);
    }
}

/// The exec stream. Losing it means the daemon is up, the bar shows a healthy
/// shield, and nothing whatsoever is being watched — so it takes the process
/// down instead, and systemd restarts it.
fn watch_execs(guard: Arc<Guard>) -> ! {
    let conn = match ProcConnector::open() {
        Ok(conn) => conn,
        Err(e) => {
            warn!("cannot subscribe to process events: {e}");
            warn!("needs root or CAP_NET_ADMIN; run under the systemd unit, or --simulate");
            std::process::exit(1);
        }
    };
    info!("watching process execs");

    let mut buf = [0u8; procconn::BUF_LEN];
    loop {
        match conn.next_exec(&mut buf) {
            Ok(Some(pid)) => guard.on_exec(pid),
            Ok(None) => {} // a fork, an exit, or something else we do not act on
            Err(e) => {
                warn!("process event stream failed: {e}");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Two synthetic detections for --simulate, once a UI is there to render them.
fn seed_simulated_detections(guard: Arc<Guard>) {
    use redcommon::json;

    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !guard.clients.any_connected() {
        if std::time::Instant::now() > deadline {
            return;
        }
        thread::sleep(Duration::from_millis(500));
    }

    let samples = [
        json::obj([
            ("exe", json::s("/tmp/.x/beacon")),
            ("name", json::s("beacon")),
            ("kind", json::s(detect::FOREIGN_EXEC)),
            ("parent", json::s("firefox")),
            (
                "detail",
                json::s("executable lives in a world-writable scratch dir: /tmp/.x/beacon"),
            ),
        ]),
        json::obj([
            ("exe", json::s("/usr/bin/bash")),
            ("name", json::s("bash")),
            ("kind", json::s(detect::REVERSE_SHELL)),
            ("parent", json::s("python3")),
            (
                "detail",
                json::s("an interpreter with its input/output wired to a network socket"),
            ),
        ]),
    ];

    for sample in samples {
        guard.inject(&sample);
        thread::sleep(Duration::from_millis(400));
    }
}

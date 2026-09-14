//! redwalld — per-application interactive outbound firewall daemon.
//!
//! The first time an executable opens a new outbound connection it is held at
//! the kernel (NFQUEUE) while the user is asked to allow or deny it. The
//! decision is remembered per-executable and persisted, so it survives reboots.
//!
//! This is a Rust rewrite of the Python daemon it replaces, drop-in on both
//! interfaces that matter: the same newline-JSON protocol on the same Unix
//! socket, and the same `rules.json` / `state.json` on disk. The nftables
//! setup still belongs to `install.sh`; this process only binds the queue.
//!
//! Why it was rewritten: attribution ran per packet and walked every
//! `/proc/<pid>/fd` on the machine to find the socket's owner, which cost a
//! continuous fraction of a core for as long as the daemon was up. That scan
//! is now shared across a burst of connections instead of repeated per packet.
//!
//! Safety posture is unchanged and deliberate — packets are accepted when the
//! firewall is disabled, when no UI is connected to answer, when one app has
//! already piled up too many held packets, and when a prompt goes unanswered.
//! The failure mode on the other side is a machine with no working network.

mod firewall;
mod netlink;
mod packet;
mod proc;
mod ui;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use redcommon::rules::{Rules, State};
use redcommon::{info, ui_sock, warn};

use firewall::Firewall;
use netlink::Nfqueue;

const QUEUE_NUM: u16 = 0;
const DEFAULT_SOCK: &str = "/run/redwall/ui.sock";
const DEFAULT_RULES: &str = "/var/lib/redwall/rules.json";

/// Slots the kernel keeps for packets we have not verdicted yet. Held packets
/// occupy them, which is what the per-app cap and the prompt timeout exist to
/// bound.
const QUEUE_MAX_LEN: u32 = 4096;

struct Args {
    sock: String,
    rules: String,
    ui_gid: u32,
    simulate: bool,
    /// Which NFQUEUE to bind. Only ever changed to try the netlink setup
    /// against a queue number no ruleset feeds, which is how this is verified
    /// without putting the machine's networking behind an untested daemon.
    queue: u16,
}

fn parse_args() -> Args {
    let mut args = Args {
        sock: DEFAULT_SOCK.to_string(),
        rules: DEFAULT_RULES.to_string(),
        ui_gid: 1000,
        simulate: false,
        queue: QUEUE_NUM,
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
            "--queue" => {
                i += 1;
                if let Some(v) = argv.get(i).and_then(|v| v.parse().ok()) {
                    args.queue = v;
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
                    "redwalld [--sock PATH] [--rules PATH] [--ui-gid GID] [--simulate]\n\
                     \n\
                     --simulate  run the UI protocol without NFQUEUE or root"
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
    redcommon::log::set_tag("redwall");
    let args = parse_args();

    let state_path = std::path::Path::new(&args.rules)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("state.json");

    let queue = if args.simulate {
        None
    } else {
        match Nfqueue::bind(args.queue, QUEUE_MAX_LEN) {
            Ok(q) => Some(q),
            Err(e) => {
                warn!("cannot bind NFQUEUE {}: {e}", args.queue);
                warn!("needs CAP_NET_ADMIN; run under the systemd unit, or --simulate");
                std::process::exit(1);
            }
        }
    };

    let fw = Arc::new(Firewall::new(
        queue,
        Rules::load(&args.rules),
        State::load(state_path),
        args.simulate,
    ));

    if !args.simulate {
        let fw_packets = Arc::clone(&fw);
        thread::Builder::new()
            .name("nfqueue".into())
            .spawn(move || packet_loop(fw_packets))
            .expect("cannot start the packet thread");
    }

    if args.simulate {
        let fw_sim = Arc::clone(&fw);
        thread::Builder::new()
            .name("simulate".into())
            .spawn(move || seed_simulated_connections(fw_sim))
            .expect("cannot start the simulation");
    }

    let fw_reaper = Arc::clone(&fw);
    thread::Builder::new()
        .name("reaper".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            fw_reaper.reap_stale();
        })
        .expect("cannot start the timeout reaper");

    info!(
        "queue {} bound, listening on {} (rules {}){}",
        args.queue,
        args.sock,
        args.rules,
        if args.simulate { ", simulate mode" } else { "" }
    );

    let clients = Arc::clone(&fw.clients);
    if let Err(e) = ui_sock::serve(Arc::new(ui::Ui::new(fw)), clients, &args.sock, args.ui_gid) {
        warn!("UI socket failed: {e}");
        std::process::exit(1);
    }
}

/// A few connections for --simulate, once a UI is there to be prompted.
fn seed_simulated_connections(fw: Arc<Firewall>) {
    use redcommon::json;

    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !fw.clients.any_connected() {
        if std::time::Instant::now() > deadline {
            return;
        }
        thread::sleep(Duration::from_millis(500));
    }

    let samples = [
        json::obj([
            ("exe", json::s("/usr/lib/firefox/firefox")),
            ("name", json::s("firefox")),
            ("dst", json::s("34.117.65.55")),
            ("port", json::n(443u32)),
        ]),
        json::obj([
            ("exe", json::s("/usr/bin/Discord")),
            ("name", json::s("Discord")),
            ("dst", json::s("162.159.130.234")),
            ("port", json::n(443u32)),
        ]),
        json::obj([
            ("exe", json::s("/opt/some-telemetry/tracker")),
            ("name", json::s("tracker")),
            ("dst", json::s("8.8.8.8")),
            ("port", json::n(4444u32)),
            ("proto", json::s("udp")),
        ]),
    ];

    for sample in samples {
        fw.inject(&sample);
        thread::sleep(Duration::from_millis(300));
    }
}

fn packet_loop(fw: Arc<Firewall>) -> ! {
    // One datagram can carry several queued packets; 64 KiB comfortably holds
    // a batch of headers at our copy range.
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        let queued = match fw.recv_packets(&mut buf) {
            Ok(q) => q,
            Err(e) => {
                warn!("netlink receive failed: {e}");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        for q in queued {
            // Anything we cannot parse or attribute goes through. This daemon
            // decides what to block, and an unknown is not a decision.
            let Some(conn) = packet::parse(&q.payload) else {
                fw.accept(q.id);
                continue;
            };
            let Some(who) = fw.attribute(conn.proto, conn.sport) else {
                fw.accept(q.id);
                continue;
            };
            fw.handle(q.id, &conn, &who);
        }
    }
}

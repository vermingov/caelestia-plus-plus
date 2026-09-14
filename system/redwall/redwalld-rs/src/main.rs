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
mod json;
mod netlink;
mod packet;
mod proc;
mod rules;
mod ui;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use firewall::Firewall;
use netlink::Nfqueue;
use rules::{Rules, State};

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
                    "redwalld [--sock PATH] [--rules PATH] [--ui-gid GID] [--simulate]\n\
                     \n\
                     --simulate  run the UI protocol without NFQUEUE or root"
                );
                std::process::exit(0);
            }
            other => eprintln!("[redwall] ignoring unknown argument {other}"),
        }
        i += 1;
    }
    args
}

fn main() {
    let args = parse_args();

    let state_path = std::path::Path::new(&args.rules)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("state.json");

    let queue = if args.simulate {
        None
    } else {
        match Nfqueue::bind(QUEUE_NUM, QUEUE_MAX_LEN) {
            Ok(q) => Some(q),
            Err(e) => {
                eprintln!("[redwall] cannot bind NFQUEUE {QUEUE_NUM}: {e}");
                eprintln!("[redwall] needs CAP_NET_ADMIN; run under the systemd unit, or --simulate");
                std::process::exit(1);
            }
        }
    };

    let fw = Arc::new(Firewall::new(
        queue,
        Rules::load(&args.rules),
        State::load(state_path),
    ));

    if !args.simulate {
        let fw_packets = Arc::clone(&fw);
        thread::Builder::new()
            .name("nfqueue".into())
            .spawn(move || packet_loop(fw_packets))
            .expect("cannot start the packet thread");
    }

    let fw_reaper = Arc::clone(&fw);
    thread::Builder::new()
        .name("reaper".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            fw_reaper.reap_stale();
        })
        .expect("cannot start the timeout reaper");

    println!(
        "[redwall] listening on {} (rules {}){}",
        args.sock,
        args.rules,
        if args.simulate { ", simulate mode" } else { "" }
    );

    if let Err(e) = ui::serve(fw, &args.sock, args.ui_gid) {
        eprintln!("[redwall] UI socket failed: {e}");
        std::process::exit(1);
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
                eprintln!("[redwall] netlink receive failed: {e}");
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

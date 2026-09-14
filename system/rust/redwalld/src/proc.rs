//! Attribution: local port -> socket inode -> pid -> executable.
//!
//! This is the whole cost of the daemon. The Python version answered every
//! packet by walking `/proc/<pid>/fd` for every process on the machine and
//! `readlink`ing each entry until it found the socket — O(processes x fds) of
//! syscalls per new connection, which is why it burned a continuous fraction
//! of a core doing nothing in particular.
//!
//! The work here is the same shape but done once for a burst rather than once
//! per packet: one scan builds an inode -> pid map for every socket on the
//! system, and it is reused until it goes stale or misses. An app opening
//! twenty connections at once costs one scan, not twenty.

use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};

use redcommon::procfs;

/// How long a socket table scan stays usable. Long enough to cover a burst of
/// connections from one app, short enough that a pid reusing an inode number
/// cannot be misattributed for any meaningful time.
const CACHE_TTL: Duration = Duration::from_millis(750);

#[derive(Debug, Clone)]
pub struct Attribution {
    pub exe: String,
    pub name: String,
    pub pid: u32,
}

pub struct Attributor {
    inode_to_pid: HashMap<u64, u32>,
    scanned_at: Option<Instant>,
}

impl Attributor {
    pub fn new() -> Self {
        Attributor {
            inode_to_pid: HashMap::new(),
            scanned_at: None,
        }
    }

    pub fn attribute(&mut self, proto: &str, sport: u16) -> Option<Attribution> {
        let inode = inode_for_local_port(proto, sport)?;
        let pid = self.pid_for_inode(inode)?;
        let exe = procfs::exe(pid)?;
        // comm is what the user recognises; the basename is the fallback for a
        // process whose comm went away between the two reads.
        let name = procfs::comm(pid)
            .unwrap_or_else(|| procfs::file_name(&exe.path).to_string());
        Some(Attribution {
            exe: exe.path,
            name,
            pid,
        })
    }

    fn pid_for_inode(&mut self, inode: u64) -> Option<u32> {
        let fresh = self.scanned_at.is_some_and(|t| t.elapsed() < CACHE_TTL);

        if fresh {
            if let Some(&pid) = self.inode_to_pid.get(&inode) {
                // Confirm the process is still there before trusting the cache
                if procfs::is_alive(pid) {
                    return Some(pid);
                }
            }
        }

        // Cache miss or stale: one pass over every process's descriptors.
        self.rescan();
        self.inode_to_pid.get(&inode).copied()
    }

    fn rescan(&mut self) {
        let mut map = HashMap::with_capacity(self.inode_to_pid.len().max(256));
        if let Ok(entries) = fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
                    continue;
                };
                let Ok(fds) = fs::read_dir(entry.path().join("fd")) else {
                    continue; // process exited, or not ours to read
                };
                for fd in fds.flatten() {
                    let Ok(target) = fs::read_link(fd.path()) else {
                        continue;
                    };
                    if let Some(inode) = procfs::socket_inode(&target.to_string_lossy()) {
                        // First writer wins: a socket has one owning process,
                        // and a dup'd fd in a child resolves to the same inode
                        map.entry(inode).or_insert(pid);
                    }
                }
            }
        }
        self.inode_to_pid = map;
        self.scanned_at = Some(Instant::now());
    }

    /// Local ports currently held by this executable, for tearing down live
    /// connections when an app is denied.
    pub fn ports_for_exe(&mut self, exe: &str) -> Vec<(String, u16)> {
        self.rescan();
        let inodes: Vec<u64> = self
            .inode_to_pid
            .iter()
            .filter(|(_, &pid)| procfs::exe(pid).is_some_and(|e| e.path == exe))
            .map(|(&inode, _)| inode)
            .collect();
        if inodes.is_empty() {
            return Vec::new();
        }

        let mut out = Vec::new();
        for proto in ["tcp", "udp"] {
            for fam in [proto.to_string(), format!("{proto}6")] {
                for (port, inode) in procfs::net_sockets(&fam) {
                    if inodes.contains(&inode) {
                        out.push((proto.to_string(), port));
                    }
                }
            }
        }
        out
    }
}

fn inode_for_local_port(proto: &str, sport: u16) -> Option<u64> {
    for fam in [proto.to_string(), format!("{proto}6")] {
        if let Some((_, inode)) = procfs::net_sockets(&fam)
            .into_iter()
            .find(|&(port, _)| port == sport)
        {
            return Some(inode);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not a correctness test — run with
    ///   cargo test --release -- --ignored --nocapture bench
    /// to compare the cost of attribution against the daemon this replaces.
    #[test]
    #[ignore]
    fn bench_attribution_burst() {
        use std::net::TcpListener;
        use std::time::Instant;

        let listeners: Vec<TcpListener> = (0..20)
            .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
            .collect();
        let ports: Vec<u16> = listeners
            .iter()
            .map(|l| l.local_addr().unwrap().port())
            .collect();

        let mut att = Attributor::new();
        att.attribute("tcp", ports[0]); // warm the scan, as a live daemon would be

        let start = Instant::now();
        let mut hits = 0;
        for _ in 0..5 {
            for &p in &ports {
                if att.attribute("tcp", p).is_some() {
                    hits += 1;
                }
            }
        }
        let elapsed = start.elapsed();
        println!(
            "rust: {} attributions in {:?} => {:.3} ms each ({} resolved)",
            ports.len() * 5,
            elapsed,
            elapsed.as_secs_f64() * 1000.0 / (ports.len() * 5) as f64,
            hits
        );
    }

    #[test]
    fn attributes_a_socket_we_opened_to_ourselves() {
        // Attribution has to work against the live /proc, not just fixtures:
        // bind a port, then look it up the way the packet path does.
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let mut att = Attributor::new();
        let who = att.attribute("tcp", port).expect("no owner for our own socket");
        assert_eq!(who.pid, std::process::id());
        assert!(who.exe.contains("redwalld"), "our test binary: {}", who.exe);
        assert!(!who.name.is_empty());
    }

    #[test]
    fn an_unused_port_attributes_to_nobody() {
        let mut att = Attributor::new();
        // Bind and drop, so the port is real but the socket is gone.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert!(att.attribute("tcp", port).is_none());
    }

    #[test]
    fn the_cache_survives_a_process_going_away() {
        let mut att = Attributor::new();
        att.rescan();
        att.inode_to_pid.insert(u64::MAX, u32::MAX); // a pid that cannot exist
        assert!(att.pid_for_inode(u64::MAX).is_none(), "rescan drops it");
    }
}

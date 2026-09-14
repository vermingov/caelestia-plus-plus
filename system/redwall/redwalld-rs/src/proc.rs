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
        let exe = fs::read_link(format!("/proc/{pid}/exe"))
            .ok()?
            .to_string_lossy()
            .into_owned();
        if exe.is_empty() {
            return None;
        }
        let name = fs::read_to_string(format!("/proc/{pid}/comm"))
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                exe.rsplit('/').next().unwrap_or(&exe).to_string()
            });
        Some(Attribution { exe, name, pid })
    }

    fn pid_for_inode(&mut self, inode: u64) -> Option<u32> {
        let fresh = self
            .scanned_at
            .is_some_and(|t| t.elapsed() < CACHE_TTL);

        if fresh {
            if let Some(&pid) = self.inode_to_pid.get(&inode) {
                // Confirm the process is still there before trusting the cache
                if fs::metadata(format!("/proc/{pid}")).is_ok() {
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
                    if let Some(inode) = socket_inode(&target.to_string_lossy()) {
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
        let mut inodes = Vec::new();
        for (&inode, &pid) in &self.inode_to_pid {
            let Ok(link) = fs::read_link(format!("/proc/{pid}/exe")) else {
                continue;
            };
            if link.to_string_lossy() == exe {
                inodes.push(inode);
            }
        }
        if inodes.is_empty() {
            return Vec::new();
        }

        let mut out = Vec::new();
        for proto in ["tcp", "udp"] {
            for fam in [proto.to_string(), format!("{proto}6")] {
                let Ok(text) = fs::read_to_string(format!("/proc/net/{fam}")) else {
                    continue;
                };
                for line in text.lines().skip(1) {
                    let Some((port, inode)) = parse_net_line(line) else {
                        continue;
                    };
                    if inodes.contains(&inode) {
                        out.push((proto.to_string(), port));
                    }
                }
            }
        }
        out
    }
}

fn socket_inode(link: &str) -> Option<u64> {
    let rest = link.strip_prefix("socket:[")?;
    rest.strip_suffix(']')?.parse().ok()
}

/// `/proc/net/*` rows are fixed-column; field 1 is `HEXADDR:HEXPORT` and
/// field 9 is the socket inode.
fn parse_net_line(line: &str) -> Option<(u16, u64)> {
    let mut fields = line.split_ascii_whitespace();
    let local = fields.nth(1)?;
    let port = u16::from_str_radix(local.rsplit(':').next()?, 16).ok()?;
    let inode = fields.nth(7)?.parse().ok()?;
    Some((port, inode))
}

fn inode_for_local_port(proto: &str, sport: u16) -> Option<u64> {
    for fam in [proto.to_string(), format!("{proto}6")] {
        let Ok(text) = fs::read_to_string(format!("/proc/net/{fam}")) else {
            continue;
        };
        for line in text.lines().skip(1) {
            if let Some((port, inode)) = parse_net_line(line) {
                if port == sport {
                    return Some(inode);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_socket_inode_link() {
        assert_eq!(socket_inode("socket:[123456]"), Some(123456));
        assert_eq!(socket_inode("/dev/null"), None);
        assert_eq!(socket_inode("socket:[abc]"), None);
    }

    #[test]
    fn parses_a_proc_net_row() {
        let line = "   1: 0100007F:1F90 00000000:0000 0A 00000000:00000000 \
                    00:00000000 00000000  1000        0 45678 1 0000000000000000 100 0 0 10 0";
        let (port, inode) = parse_net_line(line).unwrap();
        assert_eq!(port, 0x1F90); // 8080
        assert_eq!(inode, 45678);
    }

    #[test]
    fn survives_a_malformed_row() {
        assert!(parse_net_line("").is_none());
        assert!(parse_net_line("  1: garbage").is_none());
    }

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
        let ports: Vec<u16> = listeners.iter().map(|l| l.local_addr().unwrap().port()).collect();

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
    fn finds_a_real_listening_socket_on_this_machine() {
        // Attribution has to work against the live /proc, not just fixtures:
        // bind a port, then look it up the way the packet path does.
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let inode = inode_for_local_port("tcp", port).expect("socket missing from /proc/net");
        let mut att = Attributor::new();
        let pid = att.pid_for_inode(inode).expect("no pid owns our own socket");
        assert_eq!(pid, std::process::id());
    }
}

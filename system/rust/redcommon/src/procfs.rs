//! The `/proc` reads both daemons need to say who a process is.
//!
//! redwall asks after a socket's owner, redguard after a process that has just
//! exec'd, but both end up asking the same questions: what is it running, what
//! is it called, who started it, and are its descriptors wired to the network.
//!
//! Every read here answers `None` instead of failing. The subject is another
//! process and it may exit mid-read, which is ordinary rather than
//! exceptional: a daemon that treated a vanished pid as an error would spend
//! its life handling one.

use std::collections::HashSet;
use std::fs;

/// The families whose sockets can carry traffic off this machine. Unix
/// sockets are deliberately absent: they are the normal shape of desktop IPC,
/// and redguard's reverse-shell test would fire on half the session bus.
pub const NET_FAMILIES: [&str; 4] = ["tcp", "tcp6", "udp", "udp6"];

/// A process's executable, as `/proc/<pid>/exe` reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Exe {
    /// The path with any ` (deleted)` suffix removed, so it still matches a
    /// stored rule for the same binary.
    pub path: String,
    /// The binary is not on disk: unlinked after exec, or never there at all
    /// (`memfd`). Ordinary for a program being upgraded underneath itself,
    /// and the standard shape of an in-memory payload — redguard weighs it,
    /// redwall only wants the path.
    pub deleted: bool,
}

pub fn exe(pid: u32) -> Option<Exe> {
    let link = fs::read_link(format!("/proc/{pid}/exe"))
        .ok()?
        .to_string_lossy()
        .into_owned();
    if link.is_empty() {
        return None;
    }
    let deleted = link.ends_with(" (deleted)") || link.starts_with("/memfd:");
    let path = link
        .strip_suffix(" (deleted)")
        .unwrap_or(&link)
        .to_string();
    Some(Exe { path, deleted })
}

/// The short name the kernel keeps for a process — what `ps` shows, and what
/// the user recognises in a prompt. Capped at 15 characters by the kernel, so
/// it is a label and never an identity.
pub fn comm(pid: u32) -> Option<String> {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The parent, for context in a prompt ("spawned by firefox").
pub fn ppid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Field 2 is the comm in parentheses and may itself contain spaces and
    // parentheses, so the fields are only unambiguous after the *last* ')'.
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_ascii_whitespace().nth(1)?.parse().ok()
}

pub fn is_alive(pid: u32) -> bool {
    fs::metadata(format!("/proc/{pid}")).is_ok()
}

/// The supplementary groups of a process, for authorising a UI connection
/// whose primary group is not the one we grant.
pub fn supplementary_groups(pid: u32) -> Vec<u32> {
    let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) else {
        return Vec::new();
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("Groups:"))
        .map(|list| list.split_ascii_whitespace().filter_map(|g| g.parse().ok()).collect())
        .unwrap_or_default()
}

/// The inode behind an `fd` symlink, if that descriptor is a socket at all.
pub fn socket_inode(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?.strip_suffix(']')?.parse().ok()
}

/// What one of a process's descriptors points at.
pub fn fd_link(pid: u32, fd: u32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/fd/{fd}"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Every `(local port, socket inode)` the kernel lists for one family.
pub fn net_sockets(family: &str) -> Vec<(u16, u64)> {
    let Ok(text) = fs::read_to_string(format!("/proc/net/{family}")) else {
        return Vec::new(); // IPv6 disabled, or a kernel without that protocol
    };
    text.lines().skip(1).filter_map(parse_net_row).collect()
}

/// Every socket inode currently owned by a network protocol, which is how a
/// descriptor is told apart from a pipe or a unix socket.
pub fn net_socket_inodes() -> HashSet<u64> {
    NET_FAMILIES
        .iter()
        .flat_map(|fam| net_sockets(fam))
        .map(|(_, inode)| inode)
        .collect()
}

/// `/proc/net/*` rows are fixed-column: field 1 is `HEXADDR:HEXPORT` and
/// field 9 is the socket inode.
fn parse_net_row(line: &str) -> Option<(u16, u64)> {
    let mut fields = line.split_ascii_whitespace();
    let local = fields.nth(1)?;
    let port = u16::from_str_radix(local.rsplit(':').next()?, 16).ok()?;
    let inode = fields.nth(7)?.parse().ok()?;
    Some((port, inode))
}

/// The last path component, for labelling a rule or matching a binary by name.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_process_back() {
        let me = std::process::id();
        let exe = exe(me).expect("our own executable");
        assert!(exe.path.starts_with('/'));
        assert!(!exe.deleted);
        assert!(comm(me).is_some());
        assert!(ppid(me).is_some_and(|p| p > 0));
        assert!(is_alive(me));
    }

    #[test]
    fn a_vanished_process_is_absent_not_an_error() {
        // Reserved as an invalid pid by the kernel, so it can never exist.
        assert!(exe(0).is_none());
        assert!(comm(0).is_none());
        assert!(ppid(0).is_none());
        assert!(!is_alive(0));
        assert!(supplementary_groups(0).is_empty());
    }

    #[test]
    fn recognises_a_socket_descriptor() {
        assert_eq!(socket_inode("socket:[123456]"), Some(123456));
        assert_eq!(socket_inode("/dev/null"), None);
        assert_eq!(socket_inode("pipe:[99]"), None);
        assert_eq!(socket_inode("socket:[nonsense]"), None);
    }

    #[test]
    fn parses_a_proc_net_row() {
        let line = "   1: 0100007F:1F90 00000000:0000 0A 00000000:00000000 \
                    00:00000000 00000000  1000        0 45678 1 0000000000000000 100 0 0 10 0";
        assert_eq!(parse_net_row(line), Some((0x1F90, 45678)));
        assert_eq!(parse_net_row(""), None);
        assert_eq!(parse_net_row("  1: garbage"), None);
    }

    #[test]
    fn finds_a_socket_we_just_opened() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let (_, inode) = net_sockets("tcp")
            .into_iter()
            .find(|&(p, _)| p == port)
            .expect("our listener is missing from /proc/net/tcp");
        assert!(net_socket_inodes().contains(&inode));
    }

    #[test]
    fn takes_the_basename_of_a_path() {
        assert_eq!(file_name("/usr/bin/bash"), "bash");
        assert_eq!(file_name("bash"), "bash");
        assert_eq!(file_name("/"), "");
    }
}

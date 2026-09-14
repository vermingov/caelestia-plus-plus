//! What counts as suspicious, and why.
//!
//! Two detections, both deliberately narrow, because a host intrusion
//! detector that cries wolf gets switched off and then protects nothing:
//!
//!   reverse-shell   An interpreter or shell whose standard in/out/err is
//!                   wired to a *network* socket. That is the canonical
//!                   reverse shell, and it essentially never happens
//!                   otherwise: an interactive shell's stdio is a pty, a
//!                   pipeline's is a pipe.
//!
//!   foreign-exec    A process running from a world-writable scratch
//!                   directory, or from a binary that is not on disk at all
//!                   (unlinked, or memfd). The classic "dropper writes the
//!                   payload and runs it", and the in-memory-only variant.
//!
//! The parent is reported for context — "spawned by firefox" sharpens an RCE
//! story — but it is never a trigger on its own. A browser opening a link
//! legitimately runs helper shells, so lineage alone would be a false-positive
//! machine.
//!
//! The reads and the judgement are kept apart: [`inspect`] touches `/proc`,
//! [`judge`] is a pure function of what it found, so the rules that decide
//! whether a process gets frozen are testable without producing a payload.

use redcommon::procfs::{self, Exe};

/// Interpreters and shells worth a second look. A reverse shell is almost
/// always one of these, and a normal one is released the instant its stdio
/// turns out to be a pty or a pipe.
const INTERPRETERS: [&str; 29] = [
    "bash", "sh", "dash", "zsh", "ksh", "fish", "ash", "busybox",
    "python", "python2", "python3", "perl", "ruby", "php", "lua", "luajit",
    "node", "deno", "tclsh", "expect", "awk", "gawk", "mawk",
    "nc", "ncat", "netcat", "socat", "telnet", "socket",
];

/// Executables that live in scratch space as a matter of course. Their
/// presence there is normal packaging, not a dropper. Kept tight on purpose:
/// every entry here is a hole.
const SAFE_TMP_PREFIXES: [&str; 4] = [
    "/tmp/.mount_",        // AppImage FUSE mounts
    "/tmp/.org.chromium.", // Chromium and Electron sandbox helpers
    "/tmp/.io.",           // some Flatpak and portal helpers
    "/tmp/appimage",       // AppImage extraction
];

/// Directories any user can write to, which is what makes a binary found
/// running from one worth asking about.
const SCRATCH_DIRS: [&str; 4] = ["/tmp/", "/var/tmp/", "/dev/shm/", "/run/user/"];

pub const REVERSE_SHELL: &str = "reverse-shell";
pub const FOREIGN_EXEC: &str = "foreign-exec";

/// What `/proc` said about a process at one instant. A process may exit or
/// exec again while it is being read, so this is a snapshot and never a
/// promise about what is true now.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub pid: u32,
    pub comm: String,
    /// Empty when the executable could not be read at all.
    pub exe: String,
    pub deleted: bool,
    pub stdio_on_net: bool,
    pub parent: String,
}

/// A process the user should be asked about, in the shape the bar renders.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub pid: u32,
    pub exe: String,
    pub name: String,
    pub kind: &'static str,
    pub parent: String,
    pub detail: String,
}

pub fn is_interpreter(name: &str) -> bool {
    INTERPRETERS.contains(&name)
}

/// True when the executable itself sits somewhere any user could have written
/// it — the dropper signature — rather than in a packaged location.
pub fn in_scratch(exe: &str) -> bool {
    if exe.is_empty() || SAFE_TMP_PREFIXES.iter().any(|p| exe.starts_with(p)) {
        return false;
    }
    SCRATCH_DIRS.iter().any(|d| exe.starts_with(d))
}

/// The cheap test that runs on every exec on the machine. Only interpreters
/// and scratch or deleted binaries are ever looked at any closer, so the
/// overwhelming majority of processes are never touched at all.
pub fn worth_inspecting(comm: &str, exe: Option<&Exe>) -> bool {
    match exe {
        Some(exe) => {
            exe.deleted
                || in_scratch(&exe.path)
                || is_interpreter(comm)
                || is_interpreter(procfs::file_name(&exe.path))
        }
        // No readable executable: all that is left to go on is the name.
        None => is_interpreter(comm),
    }
}

/// Read everything the judgement needs, in increasing order of cost. The
/// network-stdio test reads four `/proc/net` tables, so it only runs for a
/// process that could be a reverse shell and is not already a detection.
pub fn inspect(pid: u32) -> Snapshot {
    let comm = procfs::comm(pid).unwrap_or_default();
    let exe = procfs::exe(pid);
    let (path, deleted) = match &exe {
        Some(e) => (e.path.clone(), e.deleted),
        None => (String::new(), false),
    };

    let interpreter = is_interpreter(&comm) || is_interpreter(procfs::file_name(&path));
    let stdio_on_net =
        !deleted && !in_scratch(&path) && interpreter && stdio_on_network(pid);

    let parent = procfs::ppid(pid)
        .filter(|&ppid| ppid > 0)
        .and_then(procfs::comm)
        .unwrap_or_default();

    Snapshot {
        pid,
        comm,
        exe: path,
        deleted,
        stdio_on_net,
        parent,
    }
}

/// The whole policy, as a pure function of one snapshot.
pub fn judge(snap: &Snapshot) -> Option<Detection> {
    let (kind, detail) = if snap.deleted {
        (
            FOREIGN_EXEC,
            "runs from a deleted/anonymous executable (in-memory payload)".to_string(),
        )
    } else if in_scratch(&snap.exe) {
        (
            FOREIGN_EXEC,
            format!(
                "executable lives in a world-writable scratch dir: {}",
                snap.exe
            ),
        )
    } else if snap.stdio_on_net {
        (
            REVERSE_SHELL,
            "an interpreter with its input/output wired to a network socket".to_string(),
        )
    } else {
        return None;
    };

    let name = if !snap.comm.is_empty() {
        snap.comm.clone()
    } else if !snap.exe.is_empty() {
        procfs::file_name(&snap.exe).to_string()
    } else {
        format!("pid {}", snap.pid)
    };

    Some(Detection {
        pid: snap.pid,
        exe: if snap.exe.is_empty() {
            format!("(pid {})", snap.pid)
        } else {
            snap.exe.clone()
        },
        name,
        kind,
        parent: snap.parent.clone(),
        detail,
    })
}

/// True when fd 0, 1 or 2 is a network socket rather than a pty, a pipe or a
/// unix socket.
fn stdio_on_network(pid: u32) -> bool {
    let net = procfs::net_socket_inodes();
    if net.is_empty() {
        return false;
    }
    (0..=2).any(|fd| {
        procfs::fd_link(pid, fd)
            .as_deref()
            .and_then(procfs::socket_inode)
            .is_some_and(|inode| net.contains(&inode))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot {
            pid: 4242,
            comm: "bash".into(),
            exe: "/usr/bin/bash".into(),
            deleted: false,
            stdio_on_net: false,
            parent: "firefox".into(),
        }
    }

    #[test]
    fn an_ordinary_shell_is_left_alone() {
        assert_eq!(judge(&snap()), None, "a shell with pty stdio is not a payload");
    }

    #[test]
    fn a_shell_talking_to_a_socket_is_a_reverse_shell() {
        let s = Snapshot { stdio_on_net: true, ..snap() };
        let d = judge(&s).expect("detected");
        assert_eq!(d.kind, REVERSE_SHELL);
        assert_eq!(d.name, "bash");
        assert_eq!(d.parent, "firefox", "lineage is reported, never a trigger");
    }

    #[test]
    fn a_binary_in_scratch_is_a_foreign_exec() {
        let s = Snapshot {
            comm: "beacon".into(),
            exe: "/tmp/.x/beacon".into(),
            ..snap()
        };
        let d = judge(&s).expect("detected");
        assert_eq!(d.kind, FOREIGN_EXEC);
        assert!(d.detail.contains("/tmp/.x/beacon"));
    }

    #[test]
    fn a_deleted_binary_outranks_everything_else() {
        let s = Snapshot { deleted: true, stdio_on_net: true, ..snap() };
        let d = judge(&s).expect("detected");
        assert_eq!(d.kind, FOREIGN_EXEC);
        assert!(d.detail.contains("in-memory"));
    }

    #[test]
    fn packaged_scratch_paths_are_not_droppers() {
        for safe in [
            "/tmp/.mount_Obsidian123/AppRun",
            "/tmp/.org.chromium.Chromium.abc/helper",
            "/tmp/.io.flatpak/thing",
            "/tmp/appimage_extracted/x",
        ] {
            assert!(!in_scratch(safe), "{safe} is normal packaging");
        }
        for bad in ["/tmp/x", "/var/tmp/x", "/dev/shm/x", "/run/user/1000/x"] {
            assert!(in_scratch(bad), "{bad} is scratch space");
        }
        assert!(!in_scratch("/usr/bin/bash"));
        assert!(!in_scratch(""), "an unreadable exe is not evidence");
    }

    #[test]
    fn a_nameless_process_still_produces_a_usable_prompt() {
        let s = Snapshot {
            comm: String::new(),
            exe: String::new(),
            deleted: true,
            ..snap()
        };
        let d = judge(&s).expect("detected");
        assert_eq!(d.name, "pid 4242");
        assert_eq!(d.exe, "(pid 4242)");
    }

    #[test]
    fn the_prefilter_passes_only_what_is_worth_a_closer_look() {
        let ordinary = Exe { path: "/usr/bin/gedit".into(), deleted: false };
        assert!(!worth_inspecting("gedit", Some(&ordinary)));

        let shell = Exe { path: "/usr/bin/bash".into(), deleted: false };
        assert!(worth_inspecting("bash", Some(&shell)));
        // A renamed interpreter is caught by its path, a renamed binary by comm
        assert!(worth_inspecting("mytool", Some(&shell)));
        assert!(worth_inspecting("python3", Some(&ordinary)));

        let dropped = Exe { path: "/tmp/x".into(), deleted: false };
        assert!(worth_inspecting("x", Some(&dropped)));
        let gone = Exe { path: "/usr/bin/thing".into(), deleted: true };
        assert!(worth_inspecting("thing", Some(&gone)));

        assert!(!worth_inspecting("gedit", None), "no exe, unremarkable name");
        assert!(worth_inspecting("sh", None), "no exe, but it calls itself sh");
    }

    #[test]
    fn our_own_stdio_is_not_a_network_socket() {
        // The test runner's stdio is a pipe or a terminal, never a socket, so
        // this is the false-positive case the whole detection rests on.
        assert!(!stdio_on_network(std::process::id()));
    }

    #[test]
    fn inspecting_ourselves_finds_nothing_to_report() {
        let s = inspect(std::process::id());
        assert!(s.exe.contains("redguardd"), "our test binary: {}", s.exe);
        assert!(!s.deleted);
        assert_eq!(judge(&s), None, "the test runner is not a payload");
    }
}

//! Exec events from the kernel's proc connector, in real time.
//!
//! The alternative is polling `/proc`, which both misses short-lived
//! processes and costs a scan per interval, or eBPF, which drags in a
//! toolchain and a compile step at boot. The connector is a netlink multicast
//! group the kernel writes to on every fork, exec and exit; subscribing to it
//! costs one socket and wakes this daemon only when something actually runs.
//!
//! Only exec is interesting here: a process becomes what it is going to be at
//! exec, and that is the moment to look at it.

use std::io;
use std::os::unix::io::RawFd;

const AF_NETLINK: i32 = 16;
const SOCK_DGRAM: i32 = 2;
const SOCK_CLOEXEC: i32 = 0o2_000_000;
const NETLINK_CONNECTOR: i32 = 11;

const CN_IDX_PROC: u32 = 1;
const CN_VAL_PROC: u32 = 1;
const PROC_CN_MCAST_LISTEN: u32 = 1;
const PROC_EVENT_EXEC: u32 = 0x0000_0002;

const NLMSG_DONE: u16 = 3;

// struct nlmsghdr { u32 len; u16 type; u16 flags; u32 seq; u32 pid; }
const NLMSG_HDR_LEN: usize = 16;
// struct cn_msg { struct cb_id { u32 idx; u32 val; } id; u32 seq; u32 ack; u16 len; u16 flags; }
const CN_MSG_LEN: usize = 20;
// struct proc_event { u32 what; u32 cpu; u64 timestamp_ns; union {...} }
const PROC_EVENT_HEAD_LEN: usize = 16;

const CN_ID_OFFSET: usize = NLMSG_HDR_LEN;
const PROC_EVENT_OFFSET: usize = NLMSG_HDR_LEN + CN_MSG_LEN;
const EVENT_DATA_OFFSET: usize = PROC_EVENT_OFFSET + PROC_EVENT_HEAD_LEN;

/// A datagram carries one event; 1 KiB is far more than any of them needs.
pub const BUF_LEN: usize = 1024;

#[repr(C)]
struct SockaddrNl {
    nl_family: u16,
    nl_pad: u16,
    nl_pid: u32,
    nl_groups: u32,
}

pub struct ProcConnector {
    fd: RawFd,
}

impl ProcConnector {
    /// Open the socket and ask the kernel to start sending. Needs root or
    /// CAP_NET_ADMIN — the kernel refuses the subscription otherwise, which is
    /// why the daemon says so and gives up rather than running blind.
    pub fn open() -> io::Result<ProcConnector> {
        let fd = unsafe { socket(AF_NETLINK, SOCK_DGRAM | SOCK_CLOEXEC, NETLINK_CONNECTOR) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let conn = ProcConnector { fd };

        // A zero pid lets the kernel allocate the port id, which cannot
        // collide the way binding to our own pid would if anything else in
        // this process ever opens a netlink socket.
        let addr = SockaddrNl {
            nl_family: AF_NETLINK as u16,
            nl_pad: 0,
            nl_pid: 0,
            nl_groups: CN_IDX_PROC,
        };
        let rc = unsafe {
            bind(
                conn.fd,
                &addr as *const _ as *const u8,
                std::mem::size_of::<SockaddrNl>() as u32,
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }

        conn.subscribe()?;
        Ok(conn)
    }

    fn subscribe(&self) -> io::Result<()> {
        let body = PROC_CN_MCAST_LISTEN.to_ne_bytes();
        let cn_len = CN_MSG_LEN + body.len();

        let mut msg = Vec::with_capacity(NLMSG_HDR_LEN + cn_len);
        msg.extend_from_slice(&((NLMSG_HDR_LEN + cn_len) as u32).to_ne_bytes());
        msg.extend_from_slice(&NLMSG_DONE.to_ne_bytes());
        msg.extend_from_slice(&0u16.to_ne_bytes()); // flags
        msg.extend_from_slice(&0u32.to_ne_bytes()); // seq
        msg.extend_from_slice(&0u32.to_ne_bytes()); // pid: the kernel's to fill in

        msg.extend_from_slice(&CN_IDX_PROC.to_ne_bytes());
        msg.extend_from_slice(&CN_VAL_PROC.to_ne_bytes());
        msg.extend_from_slice(&0u32.to_ne_bytes()); // seq
        msg.extend_from_slice(&0u32.to_ne_bytes()); // ack
        msg.extend_from_slice(&(body.len() as u16).to_ne_bytes());
        msg.extend_from_slice(&0u16.to_ne_bytes()); // flags
        msg.extend_from_slice(&body);

        let sent = unsafe { send(self.fd, msg.as_ptr(), msg.len(), 0) };
        if sent < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Block until the kernel says something, then report the process that
    /// exec'd. Every other event — fork, exit, uid change — reads as `None`.
    pub fn next_exec(&self, buf: &mut [u8]) -> io::Result<Option<u32>> {
        let n = unsafe { recv(self.fd, buf.as_mut_ptr(), buf.len(), 0) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(parse_exec(&buf[..n as usize]))
    }
}

impl Drop for ProcConnector {
    fn drop(&mut self) {
        unsafe { close(self.fd) };
    }
}

/// The thread group that exec'd, or None for anything else in the stream.
///
/// The event carries both the task pid and its thread group id. The thread
/// group is the process: it is what `/proc/<n>` describes as a whole and what
/// a signal addresses, and after a successful exec the two agree anyway.
fn parse_exec(data: &[u8]) -> Option<u32> {
    let claimed = u32_at(data, 0)? as usize;
    if claimed > data.len() || data.len() < EVENT_DATA_OFFSET + 8 {
        return None; // truncated, or not a netlink message at all
    }
    // Anything but the process connector's own traffic is not ours to read.
    if u32_at(data, CN_ID_OFFSET)? != CN_IDX_PROC
        || u32_at(data, CN_ID_OFFSET + 4)? != CN_VAL_PROC
    {
        return None;
    }
    if u32_at(data, PROC_EVENT_OFFSET)? != PROC_EVENT_EXEC {
        return None;
    }

    // struct exec_proc_event { pid_t process_pid; pid_t process_tgid; }
    let tgid = u32_at(data, EVENT_DATA_OFFSET + 4)?;
    // init and the idle task are never candidates, and a zero means a field we
    // misread rather than a process.
    (tgid > 1).then_some(tgid)
}

/// Netlink is native-endian, being a kernel/userspace channel on one machine.
fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_ne_bytes(bytes.try_into().ok()?))
}

extern "C" {
    fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
    fn bind(fd: i32, addr: *const u8, len: u32) -> i32;
    fn send(fd: i32, buf: *const u8, len: usize, flags: i32) -> isize;
    fn recv(fd: i32, buf: *mut u8, len: usize, flags: i32) -> isize;
    fn close(fd: i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One event as the kernel lays it out, for the parser to read back.
    fn event(what: u32, pid: u32, tgid: u32) -> Vec<u8> {
        let mut m = vec![0u8; EVENT_DATA_OFFSET + 8];
        let total = m.len() as u32;
        m[0..4].copy_from_slice(&total.to_ne_bytes());
        m[CN_ID_OFFSET..CN_ID_OFFSET + 4].copy_from_slice(&CN_IDX_PROC.to_ne_bytes());
        m[CN_ID_OFFSET + 4..CN_ID_OFFSET + 8].copy_from_slice(&CN_VAL_PROC.to_ne_bytes());
        m[PROC_EVENT_OFFSET..PROC_EVENT_OFFSET + 4].copy_from_slice(&what.to_ne_bytes());
        m[EVENT_DATA_OFFSET..EVENT_DATA_OFFSET + 4].copy_from_slice(&pid.to_ne_bytes());
        m[EVENT_DATA_OFFSET + 4..EVENT_DATA_OFFSET + 8].copy_from_slice(&tgid.to_ne_bytes());
        m
    }

    #[test]
    fn reads_the_process_out_of_an_exec_event() {
        assert_eq!(parse_exec(&event(PROC_EVENT_EXEC, 4242, 4242)), Some(4242));
    }

    #[test]
    fn reports_the_thread_group_not_the_thread() {
        // A thread that execs takes over the group; the group is what a
        // signal and a /proc lookup both address.
        assert_eq!(parse_exec(&event(PROC_EVENT_EXEC, 4243, 4242)), Some(4242));
    }

    #[test]
    fn every_other_event_in_the_stream_is_ignored() {
        const PROC_EVENT_FORK: u32 = 0x0000_0001;
        const PROC_EVENT_EXIT: u32 = 0x8000_0000;
        assert_eq!(parse_exec(&event(PROC_EVENT_FORK, 10, 10)), None);
        assert_eq!(parse_exec(&event(PROC_EVENT_EXIT, 10, 10)), None);
    }

    #[test]
    fn init_is_never_a_candidate() {
        assert_eq!(parse_exec(&event(PROC_EVENT_EXEC, 1, 1)), None);
        assert_eq!(parse_exec(&event(PROC_EVENT_EXEC, 0, 0)), None);
    }

    #[test]
    fn a_short_or_foreign_datagram_is_refused() {
        assert_eq!(parse_exec(&[]), None);
        let full = event(PROC_EVENT_EXEC, 9, 9);
        for cut in [0, 4, NLMSG_HDR_LEN, PROC_EVENT_OFFSET, full.len() - 1] {
            assert_eq!(parse_exec(&full[..cut]), None, "truncated at {cut}");
        }

        // Another connector's traffic on the same socket family
        let mut other = full.clone();
        other[CN_ID_OFFSET..CN_ID_OFFSET + 4].copy_from_slice(&7u32.to_ne_bytes());
        assert_eq!(parse_exec(&other), None);
    }

    #[test]
    fn the_kernel_accepts_the_socket_we_ask_it_for() {
        // The events themselves need CAP_NET_ADMIN, so an unprivileged run
        // cannot see one. The socket, the bind address and the subscribe
        // message are checked either way: a wrong family, a wrong address
        // size or a malformed request fails here whoever is running.
        match ProcConnector::open() {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {}
            Err(e) => panic!("the proc connector was refused for the wrong reason: {e}"),
        }
    }

    #[test]
    fn a_length_field_longer_than_the_data_is_refused() {
        let mut lying = event(PROC_EVENT_EXEC, 9, 9);
        lying[0..4].copy_from_slice(&9999u32.to_ne_bytes());
        assert_eq!(parse_exec(&lying), None);
    }
}

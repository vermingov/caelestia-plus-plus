//! NFQUEUE over a raw NETLINK_NETFILTER socket.
//!
//! Written out rather than taken from a crate for two reasons. It is a root
//! daemon sitting on every new outbound connection, so the dependency surface
//! is worth keeping at zero; and the wrappers model a queued packet as an
//! owned object that must be handed back to the queue to be verdicted, which
//! forces every verdict onto the receiving thread. Here a held packet is just
//! its `packet_id`, and a verdict is one `send` on the socket — so the UI
//! thread and the timeout reaper can answer a prompt directly, which is
//! exactly what this daemon needs to do.

use std::io;
use std::os::unix::io::RawFd;
use std::sync::Mutex;

// nlmsghdr
const NLMSG_HDR_LEN: usize = 16;
// nfgenmsg
const NFGEN_LEN: usize = 4;
// nlattr
const NLA_HDR_LEN: usize = 4;

const NETLINK_NETFILTER: i32 = 12;
const NFNL_SUBSYS_QUEUE: u16 = 3;

const NFQNL_MSG_PACKET: u16 = 0;
const NFQNL_MSG_VERDICT: u16 = 1;
const NFQNL_MSG_CONFIG: u16 = 2;

const NFQA_PACKET_HDR: u16 = 1;
const NFQA_PAYLOAD: u16 = 10;

const NFQA_VERDICT_HDR: u16 = 1;

const NFQA_CFG_CMD: u16 = 1;
const NFQA_CFG_PARAMS: u16 = 2;
const NFQA_CFG_QUEUE_MAXLEN: u16 = 3;

const NFQNL_CFG_CMD_BIND: u8 = 1;
const NFQNL_COPY_PACKET: u8 = 2;

const NLM_F_REQUEST: u16 = 1;
const NLM_F_ACK: u16 = 4;

const AF_UNSPEC: u8 = 0;
const AF_INET: u16 = 2;

pub const NF_DROP: u32 = 0;
pub const NF_ACCEPT: u32 = 1;

/// A packet the kernel is holding for us. Only the id is needed to answer it.
pub struct Queued {
    pub id: u32,
    pub payload: Vec<u8>,
}

pub struct Nfqueue {
    fd: RawFd,
    queue_num: u16,
    /// Verdicts are sent from the UI and reaper threads as well as the packet
    /// loop, so the write side is serialised. Reads stay on the packet thread.
    send_lock: Mutex<()>,
}

// The fd is used for reads on one thread and guarded writes from others.
unsafe impl Send for Nfqueue {}
unsafe impl Sync for Nfqueue {}

impl Nfqueue {
    pub fn bind(queue_num: u16, max_len: u32) -> io::Result<Self> {
        let fd = unsafe {
            libc_socket(
                libc_af_netlink(),
                libc_sock_raw() | libc_sock_cloexec(),
                NETLINK_NETFILTER,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let addr = SockaddrNl {
            nl_family: libc_af_netlink() as u16,
            nl_pad: 0,
            nl_pid: 0, // let the kernel assign
            nl_groups: 0,
        };
        let rc = unsafe {
            bind_raw(
                fd,
                &addr as *const _ as *const u8,
                std::mem::size_of::<SockaddrNl>() as u32,
            )
        };
        if rc < 0 {
            let e = io::Error::last_os_error();
            unsafe { close_raw(fd) };
            return Err(e);
        }

        // A queued packet is worthless if we cannot read it, and the kernel
        // drops messages it cannot fit; give the receive buffer real room.
        set_rcvbuf(fd, 1 << 21);

        let q = Nfqueue {
            fd,
            queue_num,
            send_lock: Mutex::new(()),
        };

        q.config_cmd(NFQNL_CFG_CMD_BIND)?;
        q.config_params(0xffff, NFQNL_COPY_PACKET)?;
        q.config_maxlen(max_len)?;
        Ok(q)
    }

    fn config_cmd(&self, command: u8) -> io::Result<()> {
        // struct nfqnl_msg_config_cmd { u8 command; u8 _pad; u16 pf; }
        let mut body = Vec::with_capacity(4);
        body.push(command);
        body.push(0);
        body.extend_from_slice(&AF_INET.to_be_bytes());
        let msg = self.build(NFQNL_MSG_CONFIG, &[(NFQA_CFG_CMD, &body)]);
        self.send_checked(&msg)
    }

    fn config_params(&self, copy_range: u32, copy_mode: u8) -> io::Result<()> {
        // struct nfqnl_msg_config_params { u32 copy_range; u8 copy_mode; }
        let mut body = Vec::with_capacity(5);
        body.extend_from_slice(&copy_range.to_be_bytes());
        body.push(copy_mode);
        let msg = self.build(NFQNL_MSG_CONFIG, &[(NFQA_CFG_PARAMS, &body)]);
        self.send_checked(&msg)
    }

    fn config_maxlen(&self, max_len: u32) -> io::Result<()> {
        let body = max_len.to_be_bytes();
        let msg = self.build(NFQNL_MSG_CONFIG, &[(NFQA_CFG_QUEUE_MAXLEN, &body)]);
        self.send_checked(&msg)
    }

    /// Answer a held packet. Safe to call from any thread, and safe to call
    /// for an id the kernel has already timed out — the kernel just ignores it.
    pub fn verdict(&self, id: u32, verdict: u32) -> io::Result<()> {
        // struct nfqnl_msg_verdict_hdr { u32 verdict; u32 id; }
        let mut body = Vec::with_capacity(8);
        body.extend_from_slice(&verdict.to_be_bytes());
        body.extend_from_slice(&id.to_be_bytes());
        let msg = self.build(NFQNL_MSG_VERDICT, &[(NFQA_VERDICT_HDR, &body)]);
        self.send(&msg)
    }

    /// Blocks until the kernel queues packets, then returns everything in the
    /// datagram. One read can carry several messages.
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<Vec<Queued>> {
        let n = unsafe { recv_raw(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(parse_packets(&buf[..n as usize]))
    }

    fn build(&self, msg_type: u16, attrs: &[(u16, &[u8])]) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.resize(NLMSG_HDR_LEN, 0);

        // struct nfgenmsg { u8 nfgen_family; u8 version; u16 res_id; }
        out.push(AF_UNSPEC);
        out.push(0); // NFNETLINK_V0
        out.extend_from_slice(&self.queue_num.to_be_bytes());

        for (ty, payload) in attrs {
            let len = NLA_HDR_LEN + payload.len();
            out.extend_from_slice(&(len as u16).to_le_bytes());
            out.extend_from_slice(&ty.to_le_bytes());
            out.extend_from_slice(payload);
            while out.len() % 4 != 0 {
                out.push(0);
            }
        }

        let total = out.len() as u32;
        out[0..4].copy_from_slice(&total.to_le_bytes());
        let ty = (NFNL_SUBSYS_QUEUE << 8) | msg_type;
        out[4..6].copy_from_slice(&ty.to_le_bytes());
        out[6..8].copy_from_slice(&(NLM_F_REQUEST | NLM_F_ACK).to_le_bytes());
        // seq and pid stay zero: the kernel accepts it and we do not match acks
        out
    }

    fn send(&self, msg: &[u8]) -> io::Result<()> {
        let _guard = self.send_lock.lock().unwrap_or_else(|e| e.into_inner());
        let n = unsafe { send_raw(self.fd, msg.as_ptr(), msg.len()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Send a setup message and wait for the kernel to say it took it.
    ///
    /// Every message carries NLM_F_ACK, and until this existed nothing ever
    /// read the answer: a rejected bind looked exactly like a successful one,
    /// the daemon settled into its receive loop, and the queue it thought it
    /// owned delivered nothing — while the nftables rule went on handing every
    /// new connection to a queue with nobody on it. The machine loses its
    /// networking and the daemon reports itself healthy. Never again silently.
    fn send_checked(&self, msg: &[u8]) -> io::Result<()> {
        self.send(msg)?;

        let mut buf = [0u8; 4096];
        let n = unsafe { recv_raw(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        let reply = &buf[..n as usize];

        // struct nlmsgerr { int error; struct nlmsghdr msg; } — a zero error
        // is the plain acknowledgement.
        const NLMSG_ERROR: u16 = 2;
        if reply.len() < NLMSG_HDR_LEN + 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "short reply to a queue setup message",
            ));
        }
        let kind = u16::from_le_bytes(reply[4..6].try_into().unwrap());
        if kind != NLMSG_ERROR {
            return Ok(()); // not an ack at all; the caller's next read sorts it
        }
        let code = i32::from_le_bytes(reply[NLMSG_HDR_LEN..NLMSG_HDR_LEN + 4].try_into().unwrap());
        if code == 0 {
            return Ok(());
        }
        Err(io::Error::from_raw_os_error(-code))
    }
}

impl Drop for Nfqueue {
    fn drop(&mut self) {
        unsafe { close_raw(self.fd) };
    }
}

/// Walks a netlink datagram and pulls out every queued packet in it.
fn parse_packets(buf: &[u8]) -> Vec<Queued> {
    let mut out = Vec::new();
    let mut off = 0usize;

    while off + NLMSG_HDR_LEN <= buf.len() {
        let len = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
        if len < NLMSG_HDR_LEN || off + len > buf.len() {
            break;
        }
        let ty = u16::from_le_bytes(buf[off + 4..off + 6].try_into().unwrap());
        let subsys = ty >> 8;
        let kind = ty & 0xff;

        if subsys == NFNL_SUBSYS_QUEUE && kind == NFQNL_MSG_PACKET {
            let body = &buf[off + NLMSG_HDR_LEN + NFGEN_LEN..off + len];
            if let Some(q) = parse_attrs(body) {
                out.push(q);
            }
        }

        off += (len + 3) & !3; // NLMSG_ALIGN
    }
    out
}

fn parse_attrs(mut body: &[u8]) -> Option<Queued> {
    let mut id = None;
    let mut payload = None;

    while body.len() >= NLA_HDR_LEN {
        let nla_len = u16::from_le_bytes(body[0..2].try_into().ok()?) as usize;
        let nla_type = u16::from_le_bytes(body[2..4].try_into().ok()?) & 0x3fff;
        if nla_len < NLA_HDR_LEN || nla_len > body.len() {
            break;
        }
        let data = &body[NLA_HDR_LEN..nla_len];

        match nla_type {
            // struct nfqnl_msg_packet_hdr { u32 packet_id; u16 hw_protocol; u8 hook; }
            NFQA_PACKET_HDR if data.len() >= 4 => {
                id = Some(u32::from_be_bytes(data[0..4].try_into().ok()?));
            }
            NFQA_PAYLOAD => payload = Some(data.to_vec()),
            _ => {}
        }

        let step = (nla_len + 3) & !3;
        if step > body.len() {
            break;
        }
        body = &body[step..];
    }

    Some(Queued {
        id: id?,
        payload: payload.unwrap_or_default(),
    })
}

fn set_rcvbuf(fd: RawFd, size: i32) {
    const SOL_SOCKET: i32 = 1;
    const SO_RCVBUFFORCE: i32 = 33;
    const SO_RCVBUF: i32 = 8;
    unsafe {
        // FORCE skips rmem_max, and we are root; fall back if the kernel says no
        if setsockopt_raw(fd, SOL_SOCKET, SO_RCVBUFFORCE, &size) < 0 {
            let _ = setsockopt_raw(fd, SOL_SOCKET, SO_RCVBUF, &size);
        }
    }
}

#[repr(C)]
struct SockaddrNl {
    nl_family: u16,
    nl_pad: u16,
    nl_pid: u32,
    nl_groups: u32,
}

// Minimal libc surface, declared here so the crate keeps no dependencies.
extern "C" {
    #[link_name = "socket"]
    fn socket_raw(domain: i32, ty: i32, protocol: i32) -> i32;
    #[link_name = "bind"]
    fn bind_raw(fd: i32, addr: *const u8, len: u32) -> i32;
    #[link_name = "send"]
    fn send_c(fd: i32, buf: *const u8, len: usize, flags: i32) -> isize;
    #[link_name = "recv"]
    fn recv_c(fd: i32, buf: *mut u8, len: usize, flags: i32) -> isize;
    #[link_name = "close"]
    fn close_raw(fd: i32) -> i32;
    #[link_name = "setsockopt"]
    fn setsockopt_c(fd: i32, level: i32, name: i32, val: *const i32, len: u32) -> i32;
}

unsafe fn libc_socket(domain: i32, ty: i32, protocol: i32) -> i32 {
    socket_raw(domain, ty, protocol)
}
unsafe fn send_raw(fd: i32, buf: *const u8, len: usize) -> isize {
    send_c(fd, buf, len, 0)
}
unsafe fn recv_raw(fd: i32, buf: *mut u8, len: usize) -> isize {
    recv_c(fd, buf, len, 0)
}
unsafe fn setsockopt_raw(fd: i32, level: i32, name: i32, val: &i32) -> i32 {
    setsockopt_c(fd, level, name, val, std::mem::size_of::<i32>() as u32)
}

fn libc_af_netlink() -> i32 {
    16
}
fn libc_sock_raw() -> i32 {
    3
}
fn libc_sock_cloexec() -> i32 {
    0o2000000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_well_formed_verdict() {
        let q = Nfqueue {
            fd: -1,
            queue_num: 0,
            send_lock: Mutex::new(()),
        };
        let mut body = Vec::new();
        body.extend_from_slice(&NF_ACCEPT.to_be_bytes());
        body.extend_from_slice(&42u32.to_be_bytes());
        let msg = q.build(NFQNL_MSG_VERDICT, &[(NFQA_VERDICT_HDR, &body)]);

        let len = u32::from_le_bytes(msg[0..4].try_into().unwrap()) as usize;
        assert_eq!(len, msg.len());
        assert_eq!(len % 4, 0);
        let ty = u16::from_le_bytes(msg[4..6].try_into().unwrap());
        assert_eq!(ty >> 8, NFNL_SUBSYS_QUEUE);
        assert_eq!(ty & 0xff, NFQNL_MSG_VERDICT);
        // nfgenmsg carries the queue number big-endian
        assert_eq!(&msg[18..20], &0u16.to_be_bytes());
        std::mem::forget(q); // fd -1 must not be closed
    }

    #[test]
    fn parses_a_packet_message_back_out() {
        // Assemble one NFQNL_MSG_PACKET the way the kernel would
        let mut inner = Vec::new();
        let hdr = {
            let mut h = Vec::new();
            h.extend_from_slice(&7u32.to_be_bytes()); // packet_id
            h.extend_from_slice(&0x0800u16.to_be_bytes());
            h.push(0);
            h
        };
        for (ty, data) in [(NFQA_PACKET_HDR, hdr.as_slice()), (NFQA_PAYLOAD, &[1, 2, 3][..])] {
            let len = NLA_HDR_LEN + data.len();
            inner.extend_from_slice(&(len as u16).to_le_bytes());
            inner.extend_from_slice(&ty.to_le_bytes());
            inner.extend_from_slice(data);
            while inner.len() % 4 != 0 {
                inner.push(0);
            }
        }

        let total = NLMSG_HDR_LEN + NFGEN_LEN + inner.len();
        let mut msg = Vec::new();
        msg.extend_from_slice(&(total as u32).to_le_bytes());
        // nlmsghdr is len(4) type(2) flags(2) seq(4) pid(4) = 16 bytes
        msg.extend_from_slice(&(((NFNL_SUBSYS_QUEUE << 8) | NFQNL_MSG_PACKET).to_le_bytes()));
        msg.extend_from_slice(&0u16.to_le_bytes()); // flags
        msg.extend_from_slice(&0u32.to_le_bytes()); // seq
        msg.extend_from_slice(&0u32.to_le_bytes()); // pid
        assert_eq!(msg.len(), NLMSG_HDR_LEN);
        msg.push(AF_UNSPEC);
        msg.push(0);
        msg.extend_from_slice(&0u16.to_be_bytes());
        msg.extend_from_slice(&inner);

        let got = parse_packets(&msg);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, 7);
        assert_eq!(got[0].payload, vec![1, 2, 3]);
    }

    #[test]
    fn ignores_a_truncated_datagram() {
        assert!(parse_packets(&[0, 1, 2]).is_empty());
        let mut bogus = 64u32.to_le_bytes().to_vec();
        bogus.resize(16, 0);
        assert!(parse_packets(&bogus).is_empty()); // claims 64 bytes, has 16
    }
}

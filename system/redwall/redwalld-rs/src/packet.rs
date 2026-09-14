//! IPv4/IPv6 header parsing, enough to identify a new outbound connection.
//!
//! Everything here reads from a slice the kernel handed us and never trusts a
//! length field it has not checked, because this is attacker-adjacent input
//! arriving as root.

use std::fmt::Write as _;

pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;

#[derive(Debug, Clone, PartialEq)]
pub struct Conn {
    pub proto: &'static str,
    pub saddr: String,
    pub sport: u16,
    pub daddr: String,
    pub dport: u16,
}

pub fn parse(payload: &[u8]) -> Option<Conn> {
    let version = payload.first()? >> 4;
    let (proto, saddr, daddr, l4) = match version {
        4 => {
            let ihl = (payload[0] & 0x0f) as usize * 4;
            if ihl < 20 || payload.len() < ihl {
                return None;
            }
            let proto = *payload.get(9)?;
            let saddr = ipv4(payload.get(12..16)?);
            let daddr = ipv4(payload.get(16..20)?);
            (proto, saddr, daddr, &payload[ihl..])
        }
        6 => {
            if payload.len() < 40 {
                return None;
            }
            // Next-header, not a full extension-header walk: these are bare
            // SYNs off the OUTPUT hook, which do not carry extension chains.
            let proto = payload[6];
            let saddr = ipv6(payload.get(8..24)?);
            let daddr = ipv6(payload.get(24..40)?);
            (proto, saddr, daddr, &payload[40..])
        }
        _ => return None,
    };

    let proto = match proto {
        PROTO_TCP => "tcp",
        PROTO_UDP => "udp",
        _ => return None,
    };
    if l4.len() < 4 {
        return None;
    }

    Some(Conn {
        proto,
        saddr,
        sport: u16::from_be_bytes([l4[0], l4[1]]),
        daddr,
        dport: u16::from_be_bytes([l4[2], l4[3]]),
    })
}

fn ipv4(b: &[u8]) -> String {
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

/// RFC 5952 presentation form: lowercase hex, longest run of zero groups
/// collapsed to `::`.
fn ipv6(b: &[u8]) -> String {
    let groups: Vec<u16> = (0..8).map(|i| u16::from_be_bytes([b[i * 2], b[i * 2 + 1]])).collect();

    let (mut best_start, mut best_len) = (usize::MAX, 0usize);
    let (mut cur_start, mut cur_len) = (usize::MAX, 0usize);
    for (i, &g) in groups.iter().enumerate() {
        if g == 0 {
            if cur_len == 0 {
                cur_start = i;
            }
            cur_len += 1;
            if cur_len > best_len {
                best_start = cur_start;
                best_len = cur_len;
            }
        } else {
            cur_len = 0;
        }
    }
    if best_len < 2 {
        best_start = usize::MAX;
    }

    let mut out = String::with_capacity(39);
    let mut i = 0;
    while i < 8 {
        if i == best_start {
            out.push_str("::");
            i += best_len;
            continue;
        }
        if !out.is_empty() && !out.ends_with(':') {
            out.push(':');
        }
        let _ = write!(out, "{:x}", groups[i]);
        i += 1;
    }
    if out.is_empty() {
        out.push_str("::");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ipv4_syn(sport: u16, dport: u16) -> Vec<u8> {
        let mut p = vec![0u8; 40];
        p[0] = 0x45; // v4, ihl 5
        p[9] = PROTO_TCP;
        p[12..16].copy_from_slice(&[192, 168, 1, 5]);
        p[16..20].copy_from_slice(&[1, 1, 1, 1]);
        p[20..22].copy_from_slice(&sport.to_be_bytes());
        p[22..24].copy_from_slice(&dport.to_be_bytes());
        p
    }

    #[test]
    fn reads_an_ipv4_tcp_connection() {
        let c = parse(&ipv4_syn(54321, 443)).unwrap();
        assert_eq!(c.proto, "tcp");
        assert_eq!(c.saddr, "192.168.1.5");
        assert_eq!(c.daddr, "1.1.1.1");
        assert_eq!(c.sport, 54321);
        assert_eq!(c.dport, 443);
    }

    #[test]
    fn reads_an_ipv6_udp_connection() {
        let mut p = vec![0u8; 48];
        p[0] = 0x60;
        p[6] = PROTO_UDP;
        p[8..24].copy_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        p[24..40].copy_from_slice(&[0x20, 0x01, 0x48, 0x60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x88, 0x88]);
        p[40..42].copy_from_slice(&1234u16.to_be_bytes());
        p[42..44].copy_from_slice(&53u16.to_be_bytes());
        let c = parse(&p).unwrap();
        assert_eq!(c.proto, "udp");
        assert_eq!(c.saddr, "2001:db8::1");
        assert_eq!(c.daddr, "2001:4860::8888");
        assert_eq!(c.dport, 53);
    }

    #[test]
    fn refuses_anything_it_cannot_vouch_for() {
        assert!(parse(&[]).is_none());
        assert!(parse(&[0x45]).is_none(), "truncated v4");
        assert!(parse(&[0x60; 8]).is_none(), "truncated v6");

        let mut icmp = ipv4_syn(1, 2);
        icmp[9] = 1; // ICMP: not ours to judge
        assert!(parse(&icmp).is_none());

        let mut bad_ihl = ipv4_syn(1, 2);
        bad_ihl[0] = 0x41; // ihl 1, below the 20-byte minimum
        assert!(parse(&bad_ihl).is_none());

        let mut v5 = ipv4_syn(1, 2);
        v5[0] = 0x55;
        assert!(parse(&v5).is_none());
    }

    #[test]
    fn collapses_zero_runs_the_standard_way() {
        let mut p = vec![0u8; 48];
        p[0] = 0x60;
        p[6] = PROTO_TCP;
        // ::1
        p[23] = 1;
        p[24..40].copy_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
        let c = parse(&p).unwrap();
        assert_eq!(c.saddr, "::1");
        assert_eq!(c.daddr, "fe80::2");
    }
}

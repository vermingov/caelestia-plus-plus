//! The seccomp filter sandrunner feeds to bwrap.
//!
//! Hand-assembled classic BPF for x86_64. A deny-list of the kernel surface
//! an unprivileged, capability-less process can still reach: the keyring,
//! bpf(), io_uring, userfaultfd, perf_event_open, ptrace — the usual
//! local-privilege-escalation primitives. Mount, module and reboot calls are
//! listed too, even though an empty capability set already stops them:
//! defence in depth if a kernel bug ever leaks a capability back.
//!
//! Three rules look at arguments: clone with CLONE_NEWUSER is refused, which
//! backs up bwrap's --disable-userns; clone3 returns ENOSYS so libc falls
//! back to the clone this filter can inspect; and the two ioctls that inject
//! keystrokes into a terminal are refused, backing up --new-session. Other
//! syscall ABIs (x32, ia32) kill the process. Everything else is allowed.
//!
//! Ported from `system/sandrunner/seccomp-gen.py`. The output is a fixed
//! blob — same bytes every time, and the test below pins them.

const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

const BPF_LD_W_ABS: u16 = 0x20;
const BPF_JEQ_K: u16 = 0x15;
const BPF_JSET_K: u16 = 0x45;
const BPF_RET_K: u16 = 0x06;

const RET_ALLOW: u32 = 0x7FFF_0000;
const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_EPERM: u32 = 0x0005_0000 | 1;
const RET_ENOSYS: u32 = 0x0005_0000 | 38;

/// Offsets into `struct seccomp_data`.
const OFF_NR: u32 = 0;
const OFF_ARCH: u32 = 4;
const OFF_ARG0: u32 = 16;
const OFF_ARG1: u32 = 24;

const NR_IOCTL: u32 = 16;
const NR_CLONE: u32 = 56;
const NR_CLONE3: u32 = 435;
const CLONE_NEWUSER: u32 = 0x1000_0000;
const TIOCSTI: u32 = 0x5412;
const TIOCLINUX: u32 = 0x541C;

/// x86_64 syscall numbers refused with EPERM, in ascending order — which is
/// the order the filter tests them in.
const DENY: [u32; 56] = [
    101, 103, 134, 135, 136, 139, 153, 155, 156, 163, 165, 166, 167, 168, 169, 170, 171, 172, 173,
    174, 175, 176, 177, 178, 179, 180, 212, 246, 248, 249, 250, 272, 298, 303, 304, 308, 310, 311,
    312, 313, 320, 321, 323, 425, 426, 427, 428, 429, 430, 431, 432, 433, 438, 442, 443, 447,
];

/// One `struct sock_filter`: opcode, the two jump offsets, and the operand.
fn instruction(code: u16, jt: u8, jf: u8, k: u32) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&code.to_le_bytes());
    out[2] = jt;
    out[3] = jf;
    out[4..8].copy_from_slice(&k.to_le_bytes());
    out
}

fn stmt(code: u16, k: u32) -> [u8; 8] {
    instruction(code, 0, 0, k)
}

fn jump(code: u16, k: u32, jt: u8, jf: u8) -> [u8; 8] {
    instruction(code, jt, jf, k)
}

pub fn build() -> Vec<u8> {
    let mut program: Vec<[u8; 8]> = vec![
        stmt(BPF_LD_W_ABS, OFF_ARCH),
        jump(BPF_JEQ_K, AUDIT_ARCH_X86_64, 1, 0),
        stmt(BPF_RET_K, RET_KILL_PROCESS),
        stmt(BPF_LD_W_ABS, OFF_NR),
        jump(BPF_JSET_K, X32_SYSCALL_BIT, 0, 1),
        stmt(BPF_RET_K, RET_KILL_PROCESS),
    ];

    for nr in DENY {
        program.push(jump(BPF_JEQ_K, nr, 0, 1));
        program.push(stmt(BPF_RET_K, RET_EPERM));
    }

    // clone3 takes its flags in a struct BPF cannot follow, so it is refused
    // outright; libc then retries with clone, which the next block inspects.
    program.push(jump(BPF_JEQ_K, NR_CLONE3, 0, 1));
    program.push(stmt(BPF_RET_K, RET_ENOSYS));

    // clone: refused only when CLONE_NEWUSER is among its flags.
    program.push(jump(BPF_JEQ_K, NR_CLONE, 0, 4));
    program.push(stmt(BPF_LD_W_ABS, OFF_ARG0));
    program.push(jump(BPF_JSET_K, CLONE_NEWUSER, 0, 1));
    program.push(stmt(BPF_RET_K, RET_EPERM));
    program.push(stmt(BPF_LD_W_ABS, OFF_NR));

    // ioctl: refused only for the two requests that push input into a tty.
    program.push(jump(BPF_JEQ_K, NR_IOCTL, 0, 5));
    program.push(stmt(BPF_LD_W_ABS, OFF_ARG1));
    program.push(jump(BPF_JEQ_K, TIOCSTI, 1, 0));
    program.push(jump(BPF_JEQ_K, TIOCLINUX, 0, 1));
    program.push(stmt(BPF_RET_K, RET_EPERM));
    program.push(stmt(BPF_LD_W_ABS, OFF_NR));

    program.push(stmt(BPF_RET_K, RET_ALLOW));
    program.concat()
}

pub fn run(args: &[String]) -> i32 {
    let [output] = args else {
        eprintln!("usage: caelestia-tools seccomp OUTPUT_FILE");
        return 2;
    };
    match std::fs::write(output, build()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("caelestia-tools seccomp: {output}: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The filter is a fixed blob, so its exact bytes are the test. This
    /// digest is the one the Python generator produces.
    #[test]
    fn the_filter_is_the_bytes_it_has_always_been() {
        let program = build();
        assert_eq!(program.len() % 8, 0, "whole instructions only");
        assert_eq!(program.len() / 8, 6 + DENY.len() * 2 + 2 + 5 + 6 + 1);

        let ours: String = program.iter().map(|b| format!("{b:02x}")).collect();
        let theirs: String =
            include_str!("../../tests/seccomp-program.hex").split_whitespace().collect();
        assert_eq!(ours, theirs, "the filter is not the one the Python generator emits");
    }

    #[test]
    fn the_deny_list_is_sorted_and_has_no_repeats() {
        let mut sorted = DENY;
        sorted.sort_unstable();
        assert_eq!(sorted, DENY, "the filter tests these in order");
        let mut seen = sorted.to_vec();
        seen.dedup();
        assert_eq!(seen.len(), DENY.len());
    }
}

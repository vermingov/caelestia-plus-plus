# The privileged daemons, in Rust

Two root daemons and the code they share.

```
redcommon/   JSON subset, UI socket server, rule store, /proc reads, log tag
redwalld/    per-application outbound firewall (NFQUEUE)   → ../redwall
redguardd/   behavioural process protection (proc connector) → ../redguard
build.sh     build one of them as the user who owns the checkout
```

Each daemon's own README — `../redwall/README.md`, `../redguard/README.md` —
covers what it does, how it is installed and how to exercise it without root.

## Why they share a crate

The two grew up as separate Python daemons and converged on the same three
problems: a newline-JSON protocol over a Unix socket the bar connects to, a
per-executable rule file that has to survive a reboot, and `/proc` reads that
say what a process is. Keeping one copy of each means the bar's two tabs speak
one dialect, a fix to the socket's peer authorisation lands in both, and
neither daemon can drift into its own idea of what a rule file looks like.

What is *not* shared is anything either one decides. redwall holds packets at
the kernel; redguard freezes processes. Those live in their own crates, and
`redcommon` never learns about verdicts, packets, or signals — it is generic
over each daemon's vocabulary instead.

## No dependencies, on purpose

Neither daemon has a single crate dependency, including for netlink. They run
as root on hot paths — redwall on every new outbound connection, redguard on
every exec on the machine — so the dependency surface is worth keeping at
zero, and the protocols they need are small enough to own outright.

## Working on them

```sh
cargo test                      # both daemons and the shared crate
cargo clippy --all-targets
./build.sh redguardd            # release build, prints the binary path
```

The tests need no root and no network. They do use real processes and real
sockets where that is the only honest way to test something: attribution binds
a port and looks itself up through `/proc/net`, and the freeze tests run a
copy of `sleep` out of a scratch directory and check the kernel actually
stopped it. The one path they cannot cover is the exec stream itself, which
needs `CAP_NET_ADMIN`; what is covered there is the parser, and that the
kernel accepts the socket the daemon asks for.

#!/usr/bin/env bash
# The copy of GPUI this shell is built against.
#
# GPUI cannot draw a lock screen: it has no window kind for
# `ext-session-lock-v1`, the protocol a locker must speak for the compositor
# to hide the desktop and to keep hiding it if the locker dies. So the shell
# builds against a copy of GPUI with that one thing added
# (`patches/gpui-session-lock.patch`, about three hundred lines), and this
# makes the copy.
#
# Nothing is downloaded when cargo has already fetched the revision the shell
# pins: that checkout is copied, which costs a disk read. A machine that has
# never built the shell has no such checkout, and cannot get one by building,
# because the `[patch]` block in Cargo.toml points at the copy this makes —
# so the build cannot start until this has run and this could not run until
# the build had. That deadlock used to be handed to the person at the
# keyboard as a note about commenting out a TOML block; now it fetches the
# revision itself, shallow, straight from the remote.
#
# Run this before the first build on a machine; `install.sh` runs it for you.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
fork=${CAE_GPUI_FORK:-${XDG_CACHE_HOME:-$HOME/.cache}/caelestia/zed-fork}
patch_file="$here/patches/gpui-session-lock.patch"

# The revision `app/Cargo.toml` pins, read from it rather than repeated here.
rev=$(sed -n 's/.*zed", rev = "\([0-9a-f]*\)".*/\1/p' "$here/app/Cargo.toml" | head -1)
[[ -n $rev ]] || { echo "vendor: no gpui revision in app/Cargo.toml" >&2; exit 1; }

if [[ -f $fork/.caelestia-patched && $(cat "$fork/.caelestia-patched") == "$rev" ]]; then
    echo "vendor: $fork is already $rev with the patch"
    exit 0
fi

# Whatever is there is either a different revision or a half-made copy, and
# either way it is about to be replaced. Build beside it and move it into
# place at the end, so an interrupted run leaves the old copy rather than a
# directory that looks finished and is not.
staging=$fork.making
rm -rf "$staging" && mkdir -p "$staging"
trap 'rm -rf "$staging"' EXIT

# A machine where cargo has never fetched a git dependency has no checkouts
# directory at all, and `find` on one that is not there fails the pipeline —
# which, under `pipefail`, used to end the script here without a word.
checkout=""
cargo_git=${CARGO_HOME:-$HOME/.cargo}/git/checkouts
if [[ -d $cargo_git ]]; then
    checkout=$(find "$cargo_git" -maxdepth 2 -type d -name "$rev*" -path '*zed*' 2>/dev/null | head -1 || true)
fi

if [[ -n $checkout ]]; then
    echo ">> Copying $checkout"
    cp -a "$checkout/." "$staging/"
else
    # Cargo.toml pins the revision the way cargo writes it — abbreviated, and
    # also the name of cargo's own checkout directory, which is why it stays
    # that way. A fetch needs all forty characters, and asking GitHub for the
    # commit is the one request that turns one into the other.
    echo ">> Fetching zed $rev (cargo has no copy yet)"
    # Parsed rather than matched: the API pretty-prints for one caller and
    # compacts for another, and a pattern written for one shape silently
    # finds nothing in the other.
    full=$(curl -fsSL --max-time 20 "https://api.github.com/repos/zed-industries/zed/commits/$rev" 2>/dev/null \
        | python3 -c 'import json,sys
try: print(json.load(sys.stdin)["sha"])
except Exception: pass' 2>/dev/null)
    if [[ -z $full ]]; then
        echo "vendor: could not resolve zed $rev to a full revision." >&2
        echo "        GitHub was unreachable or would not say. Try again, or point" >&2
        echo "        CAE_GPUI_FORK at a checkout of that revision you already have." >&2
        exit 1
    fi

    # One commit is all that comes down: a few seconds and some tens of
    # megabytes, against the gigabyte a full clone of zed would cost.
    git init --quiet "$staging"
    git -C "$staging" remote add origin https://github.com/zed-industries/zed
    if ! git -C "$staging" fetch --quiet --depth 1 origin "$full"; then
        echo "vendor: could not fetch zed $full from GitHub." >&2
        echo "        Check the network, or point CAE_GPUI_FORK at a checkout of it." >&2
        exit 1
    fi
    git -C "$staging" checkout --quiet FETCH_HEAD
    rm -rf "$staging/.git"
fi

echo ">> Applying $(basename "$patch_file")"
patch -p1 -d "$staging" < "$patch_file"
printf '%s\n' "$rev" > "$staging/.caelestia-patched"

rm -rf "$fork"
mkdir -p "$(dirname "$fork")"
mv "$staging" "$fork"
trap - EXIT
echo "vendor: $fork is ready"

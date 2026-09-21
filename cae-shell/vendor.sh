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
# Nothing is downloaded: cargo has already fetched the revision the shell
# pins, and that checkout is what is copied. Run this before the first build
# on a machine; `install.sh` runs it for you.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
fork=${CAE_GPUI_FORK:-${XDG_CACHE_HOME:-$HOME/.cache}/caelestia/zed-fork}
patch_file="$here/patches/gpui-session-lock.patch"

# The revision `app/Cargo.toml` pins, read from it rather than repeated here.
rev=$(sed -n 's/.*zed", rev = "\([0-9a-f]*\)".*/\1/p' "$here/app/Cargo.toml" | head -1)
[[ -n $rev ]] || { echo "vendor: no gpui revision in app/Cargo.toml" >&2; exit 1; }

checkout=$(find "${CARGO_HOME:-$HOME/.cargo}/git/checkouts" -maxdepth 2 -type d -name "$rev*" -path '*zed*' 2>/dev/null | head -1)
if [[ -z $checkout ]]; then
    echo "vendor: cargo has not fetched zed $rev yet." >&2
    echo "        Comment out the [patch] block in Cargo.toml, run a build to fetch it," >&2
    echo "        put the block back, and run this again." >&2
    exit 1
fi

if [[ -f $fork/.caelestia-patched && $(cat "$fork/.caelestia-patched") == "$rev" ]]; then
    echo "vendor: $fork is already $rev with the patch"
    exit 0
fi

echo ">> Copying $checkout"
rm -rf "$fork"
mkdir -p "$fork"
cp -a "$checkout/." "$fork/"

echo ">> Applying $(basename "$patch_file")"
patch -p1 -d "$fork" < "$patch_file"
printf '%s\n' "$rev" > "$fork/.caelestia-patched"
echo "vendor: $fork is ready"

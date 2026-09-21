#!/usr/bin/env bash
# Builds the caelestia++-shell package for a release. No root: the result is
# a file to attach to the release, and `cae` installs it on every machine
# whose package is older.
#
# Unlike quickshell this is not built on the machine that runs it. The plugin
# binds no Qt private API (objdump -T finds no Qt_6_PRIVATE_API symbol in any
# of its libraries), so one build keeps loading across Qt patch releases.
#
# Usage: build.sh    prints the path of the package it built
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
build_dir=${XDG_CACHE_HOME:-$HOME/.cache}/caelestia/plugin-build

mkdir -p "$build_dir"
cp "$here/PKGBUILD" "$here"/*.patch "$build_dir/"
cd "$build_dir"
makepkg --config "$here/makepkg.conf" -Cf --skipinteg >&2

ls -t "$build_dir"/caelestia++-shell-*.pkg.tar.zst | head -1

#!/bin/sh
# Copies the wit this crate binds from an ffrwd-cli checkout, and the
# ffrwd:av world the test modules export, then shows what changed.
#
#   scripts/refresh-wit.sh [--check] [ffrwd-cli checkout]
#
# The checkout defaults to ../ffrwd-cli beside this repository. With
# --check nothing is copied: the script compares the copies here against
# the checkout's and exits 1 if they differ. Line endings are ignored, since
# a Windows checkout may have CRLF where git stores LF.
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
check=0
if [ "${1:-}" = "--check" ]; then
    check=1
    shift
fi
cli=${1:-$here/../ffrwd-cli}

deps=$cli/sidecar/vendor/wasi-webgpu-wasmtime/wit/deps
av=$cli/sidecar/wit/av.wit
if [ ! -d "$deps" ] || [ ! -f "$av" ]; then
    echo "refresh-wit: $cli is not an ffrwd-cli checkout (no $deps or $av)" >&2
    exit 2
fi

if [ "$check" = 1 ]; then
    status=0
    diff -r --strip-trailing-cr "$deps" "$here/wit/deps" || status=1
    diff --strip-trailing-cr "$av" "$here/tests/modules/wit/av.wit" || status=1
    if [ "$status" = 0 ]; then
        echo "refresh-wit: the copies match $cli"
    fi
    exit "$status"
fi

rm -rf "$here/wit/deps"
mkdir -p "$here/wit/deps"
cp -R "$deps/." "$here/wit/deps/"
cp "$av" "$here/tests/modules/wit/av.wit"

echo "refresh-wit: copied from $cli ($(git -C "$cli" rev-parse --short HEAD 2>/dev/null || echo 'not a git checkout'))"
git -C "$here" --no-pager diff --stat -- wit tests/modules/wit

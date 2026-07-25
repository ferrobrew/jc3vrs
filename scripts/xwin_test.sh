#!/bin/sh
# Cross-compile and run the workspace's unit tests for x86-64 Windows under wine.
#
# cargo-xwin supplies the `wine` runner for the Windows target, but the default
# WINEPREFIX on this machine can be stale: a prefix that predates (or never ran)
# wine's prefix update has no C:\windows\system32\cryptbase.dll, so advapi32's
# forwarded SystemFunction036 (RtlGenRandom, which std pulls in to seed hash
# maps) resolves to nothing and wine aborts with "unimplemented function".
# Point wine at a disposable prefix under target/ instead; wine creates and
# populates it on first use, and deleting it is always safe.
set -e
DIR="$(cd "$(dirname "$0")/.." && pwd)"
WINEPREFIX="${JC3VRS_TEST_WINEPREFIX:-$DIR/target/wine-test-prefix}"
export WINEPREFIX
export WINEDEBUG="${WINEDEBUG:--all}"
mkdir -p "$WINEPREFIX"
cd "$DIR"
cargo xwin test --xwin-cache-dir .xwin --target x86_64-pc-windows-msvc "$@"

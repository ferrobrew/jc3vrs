#!/bin/sh
# Cross-compile the payload + injector for x86-64 Windows and run the injector
# under wine. The injector loads jc3vrs_payload.dll from its own directory, so
# both crates build into the same target dir.
set -e
cargo xwin build --xwin-cache-dir .xwin --target x86_64-pc-windows-msvc -p jc3vrs_payload
cargo xwin build --xwin-cache-dir .xwin --target x86_64-pc-windows-msvc -p jc3vrs_injector
wine ./target/x86_64-pc-windows-msvc/debug/jc3vrs_injector.exe "$@"

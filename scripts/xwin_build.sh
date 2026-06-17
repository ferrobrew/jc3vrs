#!/bin/sh
# Cross-compile the payload + injector for x86-64 Windows without running them.
set -e
cargo xwin build --xwin-cache-dir .xwin --target-dir target-xwin --target x86_64-pc-windows-msvc -p jc3vrs_payload
cargo xwin build --xwin-cache-dir .xwin --target-dir target-xwin --target x86_64-pc-windows-msvc -p jc3vrs_injector

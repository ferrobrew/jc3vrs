#!/bin/sh
set -e
cargo xwin clippy --xwin-cache-dir .xwin --target x86_64-pc-windows-msvc --all --all-targets -- -D warnings

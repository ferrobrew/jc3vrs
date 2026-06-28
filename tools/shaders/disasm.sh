#!/bin/sh
# Disassemble a DXBC shader blob to SM5 assembly, via D3DDisassemble under wine.
#
#   disasm.sh <file.dxbc>          # prints assembly to stdout (CRLF stripped)
#
# On first use it builds disasm.exe with clang + the repo's xwin sysroot (.xwin/xwin, created by
# cargo-xwin -- run scripts/xwin_build.sh once if it is missing), and it reuses the d3dcompiler_47.dll
# and wine prefix that `cargo run -p shadergen --target x86_64-unknown-linux-gnu` provisions under
# target/fsr-shader-build/.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
fsr="$repo/target/fsr-shader-build"
xwin="$repo/.xwin/xwin"
exe="$here/disasm.exe"
dll="$fsr/d3dcompiler_47.dll"
prefix="$fsr/wineprefix"

if [ ! -f "$dll" ] || [ ! -d "$prefix" ]; then
    echo "disasm.sh: missing $fsr -- run 'cargo run -p shadergen --target x86_64-unknown-linux-gnu' once to provision d3dcompiler_47.dll + the wine prefix" >&2
    exit 1
fi
if [ ! -d "$xwin" ]; then
    echo "disasm.sh: missing $xwin -- run scripts/xwin_build.sh once to populate the xwin sysroot" >&2
    exit 1
fi

# (Re)build the harness when the source is newer. Use an unwrapped clang (the nix cc-wrapper injects
# -fPIC, which the msvc target rejects); cargo-xwin's clang-cl wrapper records the path to one.
if [ ! -f "$exe" ] || [ "$here/disasm.c" -nt "$exe" ]; then
    clang=$(grep -oE '/nix/store/[^ ]*clang-[0-9.]+/bin/clang' "$HOME/.cache/cargo-xwin/clang-cl" 2>/dev/null | head -1 || true)
    clang=${clang:-clang}
    "$clang" --target=x86_64-pc-windows-msvc -fuse-ld=lld -nostdinc \
        -isystem "$xwin/crt/include" -isystem "$xwin/sdk/include/ucrt" \
        -isystem "$xwin/sdk/include/um" -isystem "$xwin/sdk/include/shared" \
        -L"$xwin/crt/lib/x86_64" -L"$xwin/sdk/lib/um/x86_64" -L"$xwin/sdk/lib/ucrt/x86_64" \
        "$here/disasm.c" -o "$exe" -lkernel32 -lmsvcrt -llibcmt -lucrt
fi

# d3dcompiler_47.dll must sit beside the exe for the runtime LoadLibrary to find it.
[ -f "$here/d3dcompiler_47.dll" ] || cp "$dll" "$here/d3dcompiler_47.dll"

WINEPREFIX="$prefix" WINEDEBUG=-all wine "$exe" "$1" 2>/dev/null | sed 's/\r$//'

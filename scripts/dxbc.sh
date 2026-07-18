#!/bin/sh
# Compile HLSL to DXBC, or disassemble DXBC to SM5 assembly, via d3dcompiler_47 under wine.
#
#   dxbc.sh compile <file.hlsl> <entry> <target>   # writes the DXBC blob to stdout
#   dxbc.sh disasm  <file.dxbc>                     # writes SM5 assembly to stdout
#
#   scripts/dxbc.sh compile ref.hlsl main vs_5_0 > ref.dxbc
#   scripts/dxbc.sh disasm Shaders_F.shaders/sh_0467_0016b270.dxbc | less
#
# A thin wrapper around the `dxbc-tool` crate (tools/dxbc-tool), which calls D3DCompile /
# D3DDisassemble through the `windows` crate. This replaced the hand-written compile.c / disasm.c
# harnesses. It cross-builds the tool with cargo-xwin and runs the resulting exe under wine, against
# the native d3dcompiler_47.dll that `cargo run -p shadergen --target x86_64-unknown-linux-gnu`
# provisions under target/fsr-shader-build/.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
prefix="$repo/target/fsr-shader-build/wineprefix"
exe="$repo/target/x86_64-pc-windows-msvc/debug/dxbc-tool.exe"

if [ ! -d "$prefix/drive_c/windows/system32" ] ||
    [ ! -f "$prefix/drive_c/windows/system32/d3dcompiler_47.dll" ]; then
    echo "dxbc.sh: missing the provisioned wine prefix + native d3dcompiler_47.dll -- run 'cargo run -p shadergen --target x86_64-unknown-linux-gnu' once" >&2
    exit 1
fi

cargo xwin build --xwin-cache-dir "$repo/.xwin" --target x86_64-pc-windows-msvc -p dxbc-tool >&2

# `n` selects the native (provisioned) d3dcompiler_47 over wine's built-in reimplementation, which
# mis-parses some shaders. The DLL is already installed in the prefix's system32 by shadergen.
WINEPREFIX="$prefix" WINEDEBUG=-all WINEDLLOVERRIDES="d3dcompiler_47=n" wine "$exe" "$@"

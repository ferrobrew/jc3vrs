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
# harnesses. It runs against the native d3dcompiler_47.dll in the shared prefix, because wine's
# built-in reimplementation mis-parses some of the game's shaders.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
. "$here/wine_prefix.sh"
jc3vr_require_d3dcompiler

exe="$repo/target/x86_64-pc-windows-msvc/debug/dxbc-tool.exe"
cargo xwin build --xwin-cache-dir "$repo/.xwin" --target x86_64-pc-windows-msvc -p dxbc-tool >&2

wine "$exe" "$@"

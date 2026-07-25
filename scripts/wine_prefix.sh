#!/bin/sh
# The project's single wine prefix, shared by every tool that runs a Windows binary on Linux.
#
# Source this, then call `jc3vrs_ensure_wine_prefix` before invoking wine:
#
#   . "$(dirname "$0")/wine_prefix.sh"
#   jc3vrs_ensure_wine_prefix
#   wine "$some_exe"
#
# It exports `WINEPREFIX` (and quiets `WINEDEBUG` unless the caller set it), so anything run
# afterwards lands in the same prefix.
#
# There is one prefix rather than one per tool because the alternative drifts: the tools have
# different requirements, and a prefix that satisfies only some of them fails in ways that look like
# a bug in the caller. Two requirements matter.
#
# **A prefix wine has actually initialised.** A stale or hand-made prefix can be missing
# `cryptbase.dll`, which `advapi32`'s forwarded `SystemFunction036` (`RtlGenRandom`, pulled in by the
# Rust test harness) resolves through -- without it wine aborts before `main` with "unimplemented
# function". Creating it under `target/` keeps it disposable and out of `$HOME`.
#
# **A native `d3dcompiler_47.dll`.** Wine's built-in reimplementation mis-parses some of the game's
# shaders, so anything reading real DXBC needs the native DLL and a `WINEDLLOVERRIDES` that selects
# it. `shadergen` already knows how to fetch and install it, and honours `WINEPREFIX`, so it is what
# provisions the prefix here.
#
# The game's own Proton prefix is deliberately *not* this one: it belongs to Steam, has a different
# lifetime, and is reached through the container (see `proton_run.sh`).
#
# Override the location with `JC3VRS_WINEPREFIX`. Deleting the prefix is always safe -- the next
# `jc3vrs_ensure_wine_prefix` rebuilds it.

jc3vrs_wine_prefix_repo=$(cd "$(dirname "$0")/.." && pwd)
WINEPREFIX="${JC3VRS_WINEPREFIX:-$jc3vrs_wine_prefix_repo/target/wine}"
export WINEPREFIX
export WINEDEBUG="${WINEDEBUG:--all}"

# Selects the native (provisioned) `d3dcompiler_47` over wine's built-in. Harmless for callers that
# never load it, so it is set unconditionally rather than per tool.
export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-d3dcompiler_47=n}"

# Create and provision the prefix if it is not usable yet. Idempotent, and a no-op once provisioned.
#
# Deliberately does not shell out to `shadergen` to do this: that would rebuild the FSR shader
# headers as a side effect, which invalidates `fsr-sys`'s build inputs, and `fsr-sys` only compiles
# inside `nix-shell shell.nix` (it needs `clang-cl`). Provisioning has to be cheap and side-effect
# free, so it boots the prefix directly and takes the native DLL from wherever it can be had.
# Whether a `d3dcompiler_47.dll` is the *native* one rather than wine's built-in reimplementation.
#
# Existence is not the test. `wineboot` puts a built-in stub at the same path, and
# `WINEDLLOVERRIDES=d3dcompiler_47=n` then refuses to load it -- so an existence check passes, the
# prefix looks provisioned, and the caller fails later with an exit code and no output, which is
# indistinguishable from a bug in the caller. Size is the discriminator that needs no wine invocation:
# the built-in is a few hundred KB, the native DLL a few MB.
_jc3vrs_is_native_d3dcompiler() {
    [ -f "$1" ] || return 1
    _jc3vrs_size=$(wc -c <"$1" 2>/dev/null || echo 0)
    [ "$_jc3vrs_size" -ge 1048576 ]
}

jc3vrs_ensure_wine_prefix() {
    _jc3vrs_sys32="$WINEPREFIX/drive_c/windows/system32"
    if _jc3vrs_is_native_d3dcompiler "$_jc3vrs_sys32/d3dcompiler_47.dll"; then
        return 0
    fi

    if [ ! -d "$_jc3vrs_sys32" ]; then
        echo "wine prefix: creating $WINEPREFIX" >&2
        mkdir -p "$WINEPREFIX"
        wineboot -i >/dev/null 2>&1 || true
    fi

    # `shadergen` downloads and installs the native DLL; reuse its copy if a previous run left one,
    # rather than downloading a second time.
    for _jc3vrs_donor in \
        "$jc3vrs_wine_prefix_repo/target/fsr-shader-build/wineprefix/drive_c/windows/system32/d3dcompiler_47.dll" \
        "${D3DCOMPILER_DLL:-}"; do
        if [ -n "$_jc3vrs_donor" ] && _jc3vrs_is_native_d3dcompiler "$_jc3vrs_donor"; then
            cp "$_jc3vrs_donor" "$_jc3vrs_sys32/d3dcompiler_47.dll"
            echo "wine prefix: installed d3dcompiler_47 from $_jc3vrs_donor" >&2
            return 0
        fi
    done

    # Nothing to copy. Tests do not need the DLL, so this is a warning rather than a failure; the
    # tools that do need it check for themselves.
    echo "wine prefix: no native d3dcompiler_47 available -- run 'nix-shell shell.nix --run \"cargo run -p shadergen --target x86_64-unknown-linux-gnu\"' once to fetch it" >&2
}

# Fail loudly for the callers that genuinely cannot work without the native compiler.
jc3vrs_require_d3dcompiler() {
    jc3vrs_ensure_wine_prefix
    if ! _jc3vrs_is_native_d3dcompiler "$WINEPREFIX/drive_c/windows/system32/d3dcompiler_47.dll"; then
        echo "error: this tool needs the native d3dcompiler_47.dll (wine's built-in mis-parses some shaders)" >&2
        echo "       run: nix-shell shell.nix --run 'cargo run -p shadergen --target x86_64-unknown-linux-gnu'" >&2
        exit 1
    fi
}

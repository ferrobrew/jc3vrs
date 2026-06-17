{ pkgs ? import <nixpkgs> { } }:

# Development shell for cross-compiling jc3vrs to x86-64 Windows from Linux.
#
# We rely on the system `rustup`/`cargo` toolchain (with the
# `x86_64-pc-windows-msvc` target installed) and `cargo-xwin`, which drives
# LLVM's clang-cl/lld-link against the Windows SDK + CRT headers it downloads.
#
# Build with `scripts/xwin_run.sh` (or `cargo xwin build --target
# x86_64-pc-windows-msvc`) and run the resulting injector under wine.
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    # Wrapped clang first: it provides the `clang`/`clang++` used as the host
    # linker for build-scripts & proc-macros (it knows where NixOS keeps glibc's
    # crt objects, which the unwrapped clang does not). It ships no `clang-cl`,
    # so the unwrapped clang below still wins for the Windows compiler.
    clang

    # cargo-xwin and the unwrapped LLVM tools it shells out to for the Windows
    # target: clang-cl (compiler), lld-link (linker), llvm-lib / llvm-dlltool
    # (import libs). Order matters — these must come after wrapped clang.
    cargo-xwin
    llvmPackages.clang-unwrapped
    llvmPackages.bintools-unwrapped
    lld

    # Run the injector (and, by extension, the target game) under wine.
    wineWow64Packages.stable

    pkgconf
  ];

  # cargo-xwin caches the downloaded Windows SDK here; keep it inside the repo
  # so it survives shell restarts and matches the scripts' --xwin-cache-dir.
  XWIN_CACHE_DIR = ".xwin";
}

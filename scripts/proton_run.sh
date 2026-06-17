#!/usr/bin/env bash
# Build the payload + injector for x86-64 Windows and inject into a running
# Just Cause 3 by sending the injector *into the game's existing Proton
# container*, so it shares the game's wineserver and can see/inject the process.
#
# This requires the game to have been launched with a launcher service in its
# container — i.e. via jc3boot/scripts/proton_run.sh (which runs
# `steam-runtime-launcher-service --bus-name=$BUS`). We connect to that service
# with `steam-runtime-launch-client` and run the injector there via
# `proton runinprefix`, which attaches to the already-running wineserver
# instead of spinning up a private one.
#
# Why not just `proton run` ourselves: pressure-vessel gives every Proton
# session a private /tmp, so the wineserver socket (/tmp/.wine-$UID) isn't
# shared and a separate session sees no processes ("No processes found").
# Going through the launcher service lands us in the *same* container.
#
# Bonus: launch-client forwards stdio, so unlike `proton run` you actually see
# the injector's output. Re-run this script to re-inject during a session.
#
# On NixOS the launch-client runs under `steam-run` for the FHS env it needs.
#
# Overridable via env: STEAM_ROOT, PROTON_DIR, JC3_BUS_NAME.
# Any args passed to this script are forwarded to the injector.
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
STEAM="${STEAM_ROOT:-$HOME/.steam/steam}"
PROTON="${PROTON_DIR:-$STEAM/steamapps/common/Proton - Experimental}"
BUS="${JC3_BUS_NAME:-com.jc3vrs.JustCause3}"
LAUNCH_CLIENT="$STEAM/steamapps/common/SteamLinuxRuntime_sniper/pressure-vessel/bin/steam-runtime-launch-client"
INJECTOR="$DIR/target/x86_64-pc-windows-msvc/debug/jc3vrs_injector.exe"

# Build with the cross toolchain (cargo-xwin lives in the nix shell, not under
# steam-run/Proton — keep the two invocations separate).
nix-shell "$DIR/shell.nix" --run \
  "cd '$DIR' && cargo xwin build --xwin-cache-dir .xwin --target x86_64-pc-windows-msvc -p jc3vrs_payload -p jc3vrs_injector"

# Send the injector into the game's container. The command inherits the
# container's environment (STEAM_COMPAT_DATA_PATH etc.), so `proton runinprefix`
# targets the right prefix and connects to the running game's wineserver.
exec steam-run "$LAUNCH_CLIENT" --bus-name="$BUS" -- \
  "$PROTON/proton" runinprefix "$INJECTOR" "$@"

#!/usr/bin/env bash
# Launch Just Cause 3 with the VR-specific environment jc3vrs needs, then hand off
# to jc3boot's neutral launcher. jc3boot stays service-agnostic (it just spawns
# the game inside the pressure-vessel container with a launcher service so a
# payload can inject); the VR runtime policy lives here, with the VR mod.
#
# Usage: run this to launch the game, then inject with scripts/proton_run.sh as
# usual. Point JC3BOOT_DIR at your jc3boot checkout if it is not the default.
#
# Any arguments are forwarded to jc3boot's launcher (e.g. --gamescope).
set -euo pipefail

# Default to a jc3boot checkout sitting alongside this repo; override with JC3BOOT_DIR.
DIR="$(cd "$(dirname "$0")/.." && pwd)"
JC3BOOT="${JC3BOOT_DIR:-$DIR/../jc3boot}"
LAUNCHER="$JC3BOOT/scripts/proton_run.sh"
[ -x "$LAUNCHER" ] || { echo "vr_launch.sh: jc3boot launcher not found at $LAUNCHER (set JC3BOOT_DIR)" >&2; exit 1; }

# Expose the host OpenXR runtime (e.g. WiVRn) inside the pressure-vessel
# container, so Proton's Wine->host OpenXR bridge (wineopenxr.so, which NEEDs the
# host libopenxr_loader.so.1) can resolve it -- on NixOS that loader lives in
# /nix/store, off the container's default search path. pressure-vessel reads
# PRESSURE_VESSEL_* from its ambient environment, which jc3boot's launcher (via
# steam-run) inherits, so setting it here is enough; jc3boot need not name it.
export PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES=1

# Force Proton's startup OpenVR probe through xrizer, so our OpenXR path works.
#
# Proton's steam_helper runs an OpenVR probe at container startup and records the
# outcome in the prefix registry (HKCU\Software\Wine\VR "state"). wineopenxr --
# the Wine->host OpenXR bridge our payload loads -- blocks on that state and
# returns XR_ERROR_INITIALIZATION_FAILED until the probe succeeds (Linux SteamVR's
# xrCreateInstance hangs if SteamVR was never brought up, so the bridge gates on
# OpenVR having come up first). The OpenVR probe is therefore *required*, not
# redundant: if it fails, our OpenXR bring-up fails with it, before any native
# OpenXR call is ever made.
#
# The probe must resolve to xrizer (the OpenVR->OpenXR shim over whichever OpenXR
# runtime is active -- Monado, WiVRn), not SteamVR. Once SteamVR is installed
# (Monado's Lighthouse driver needs it) it registers itself first in
# openvrpaths.vrpath and shadows xrizer, so the probe tries -- and fails on -- a
# SteamVR that is not running. Point VR_OVERRIDE straight at xrizer's runtime dir
# so the probe deterministically brings up xrizer -> the active runtime and sets
# the state wineopenxr waits for. Honour a caller-set VR_OVERRIDE; otherwise read
# xrizer's path out of openvrpaths.vrpath, so there is no hardcoded store path.
if [ -z "${VR_OVERRIDE:-}" ]; then
  OPENVR_PATHS="${XDG_CONFIG_HOME:-$HOME/.config}/openvr/openvrpaths.vrpath"
  XRIZER_RT="$(grep -oE '"[^"]*xrizer[^"]*/lib/xrizer"' "$OPENVR_PATHS" 2>/dev/null | tr -d '"' | head -1)"
  if [ -n "$XRIZER_RT" ] && [ -d "$XRIZER_RT" ]; then
    export VR_OVERRIDE="$XRIZER_RT"
  else
    echo "vr_launch.sh: could not find the xrizer OpenVR runtime in $OPENVR_PATHS;" >&2
    echo "  VR bring-up will likely fail -- wineopenxr gates on a successful OpenVR probe." >&2
    echo "  Set VR_OVERRIDE to xrizer's runtime dir, or ensure xrizer is registered." >&2
  fi
fi

# steam-run scrubs the ambient environment before the sniper entry point, so
# PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES (read by pressure-vessel itself) never
# reaches it via a plain export; jc3boot must pass it in its explicit env list.
# Name the variables for jc3boot to forward there.
export JC3BOOT_FORWARD_ENV="PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES${VR_OVERRIDE:+ VR_OVERRIDE}"

exec "$LAUNCHER" "$@"

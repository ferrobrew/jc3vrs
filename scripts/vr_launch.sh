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

# Optionally suppress the OpenVR path. xrizer is registered as the system OpenVR
# runtime (~/.config/openvr/openvrpaths.vrpath), so anything Proton probes for
# OpenVR brings it up; jc3vrs uses OpenXR directly, so OpenVR is redundant. In
# practice xrizer and jc3vrs coexist fine, so this is OFF by default -- steering
# the OpenVR loader at an empty directory (which is what setting it did) also
# breaks jc3vrs's OpenXR reachability, so it is not a free suppression. Enable
# with JC3VRS_SUPPRESS_OPENVR=1 only if an OpenVR/xrizer conflict actually
# appears (jc3vrs bring-up failing with XR_ERROR_LIMIT_REACHED).
if [ -n "${JC3VRS_SUPPRESS_OPENVR:-}" ]; then
  NO_OPENVR_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/jc3vrs/no-openvr"
  mkdir -p "$NO_OPENVR_DIR"
  export VR_OVERRIDE="$NO_OPENVR_DIR"
fi

# steam-run scrubs the ambient environment before the sniper entry point, so
# PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES (read by pressure-vessel itself) never
# reaches it via a plain export; jc3boot must pass it in its explicit env list.
# Name the variables for jc3boot to forward there.
export JC3BOOT_FORWARD_ENV="PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES${VR_OVERRIDE:+ VR_OVERRIDE}"

exec "$LAUNCHER" "$@"

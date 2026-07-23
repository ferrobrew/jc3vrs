#!/bin/sh
# Fetch the official Khronos OpenXR loader and stage it beside the built payload.
#
# The payload loads the OpenXR loader dynamically at runtime, from an `openxr_loader.dll` next to
# `jc3vrs_payload.dll` (the `openxr` crate's `static` feature does not cross-build on Linux -- see the
# note in `payload/Cargo.toml`). This script provides that DLL from a pinned, checksum-verified
# Khronos release, so it never has to be staged by hand and a clean rebuild can restore it in one
# command. The download is cached under `.openxr/`; only the first run needs the network.
#
#   scripts/fetch_openxr_loader.sh    # stage into every built target/.../{debug,release} dir
set -eu

# Pinned Khronos OpenXR-SDK release. The `OpenXR.Loader` NuGet package is the lean asset (just the
# binaries); the x64 desktop loader lives at the member below. Bump both together, from
# https://github.com/KhronosGroup/OpenXR-SDK-Source/releases.
VERSION=1.1.61
SHA256=f9d5b5fee7038c470c6f80cf7bec1b802b55050c11d4f11179dae36c10082dae
URL="https://github.com/KhronosGroup/OpenXR-SDK-Source/releases/download/release-${VERSION}/OpenXR.Loader.${VERSION}.nupkg"
MEMBER="native/x64/release/bin/openxr_loader.dll"

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
cache="$repo/.openxr"
pkg="$cache/OpenXR.Loader.${VERSION}.nupkg"
dll="$cache/openxr_loader.dll"

mkdir -p "$cache"

# Download once, verified against the pinned hash. Re-fetch if the cached package is missing or its
# hash no longer matches (a partial or corrupt download).
if [ ! -f "$pkg" ] || ! printf '%s  %s\n' "$SHA256" "$pkg" | sha256sum -c - >/dev/null 2>&1; then
    echo "fetch_openxr_loader: downloading the OpenXR loader $VERSION" >&2
    curl -fSL --retry 3 -o "$pkg" "$URL"
    printf '%s  %s\n' "$SHA256" "$pkg" | sha256sum -c - >/dev/null || {
        echo "fetch_openxr_loader: checksum mismatch for $pkg (expected $SHA256)" >&2
        exit 1
    }
fi

# Extract the x64 desktop loader from the package (a NuGet package is a zip; there is no `unzip` in the
# dev shell, so use Python, which the other tooling already relies on).
python3 - "$pkg" "$MEMBER" "$dll" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as z:
    data = z.read(sys.argv[2])
with open(sys.argv[3], "wb") as f:
    f.write(data)
PY

# Stage beside the payload in every built target profile.
staged=0
for profile in debug release; do
    dir="$repo/target/x86_64-pc-windows-msvc/$profile"
    [ -d "$dir" ] || continue
    cp "$dll" "$dir/openxr_loader.dll"
    echo "fetch_openxr_loader: staged -> $dir/openxr_loader.dll" >&2
    staged=1
done
if [ "$staged" = 0 ]; then
    echo "fetch_openxr_loader: no built target dir yet; the loader is cached at $dll" >&2
fi

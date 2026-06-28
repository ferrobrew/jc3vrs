#!/usr/bin/env python3
"""Extract DXBC shader blobs from a Just Cause 3 ``.shader_bundle``.

The bundles are ADF containers that pack one DXBC blob per shader permutation. This carves them out by
scanning for the ``DXBC`` container magic and reading each container's embedded total-size field (a
u32 at offset 0x18), which avoids needing to parse the ADF structure.

Usage::

    extract_dxbc.py <bundle.shader_bundle> [output_dir]

``output_dir`` defaults to ``./<bundle-stem>.shaders/``. Files are named ``sh_<index>_<offset>.dxbc``,
where ``<offset>`` is the blob's byte offset in the bundle (handy for cross-referencing).
"""

import os
import struct
import sys


def extract(src: str, out: str) -> int:
    data = open(src, "rb").read()
    os.makedirs(out, exist_ok=True)
    i = n = 0
    sizes = []
    while True:
        j = data.find(b"DXBC", i)
        if j < 0:
            break
        # DXBC container header: 'DXBC'(4) digest(16) version(4) total_size(u32 @0x18) chunk_count(4).
        if j + 0x20 > len(data):
            break
        total = struct.unpack_from("<I", data, j + 0x18)[0]
        if total <= 0 or total > 4_000_000 or j + total > len(data):
            i = j + 4  # not a real container (or a 'DXBC' byte sequence inside data); skip past it.
            continue
        open(os.path.join(out, f"sh_{n:04d}_{j:08x}.dxbc"), "wb").write(data[j : j + total])
        sizes.append(total)
        n += 1
        i = j + total
    if n:
        print(f"extracted {n} blobs to {out}/ ({sum(sizes)} bytes; min {min(sizes)}, max {max(sizes)})")
    return n


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(f"usage: {sys.argv[0]} <bundle.shader_bundle> [output_dir]")
    src = os.path.expanduser(sys.argv[1])
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.splitext(os.path.basename(src))[0] + ".shaders"
    if not extract(src, out):
        sys.exit("no DXBC blobs found (is this a JC3 .shader_bundle?)")


if __name__ == "__main__":
    main()

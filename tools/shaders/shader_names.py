#!/usr/bin/env python3
"""Name the shaders in a Just Cause 3 ``.shader_bundle``.

The bundle is an ADF container holding a single ``ShaderLibrary`` instance, which carries six arrays
of ``Shader`` records — one per program type. Each record names a shader and points at its DXBC blob,
so the bundle names every blob it contains; see ``docs/engine/rendering/shaders.md`` for the layout.

If the ADF header does not parse (a truncated file, or a bundle from another build whose header
differs), this falls back to a backwards scan for the null-padded ASCII that precedes each ``DXBC``
magic. The scan is incomplete — it misses a couple of hundred blobs in a stock bundle — so the mode
that ran is always reported.

Usage::

    shader_names.py <bundle.shader_bundle> [--json] [--scan]

Output is one ``<index> <offset> <type> <name>`` row per blob, ordered by byte offset, so ``<index>``
and ``<offset>`` line up with the ``sh_<index>_<offset>.dxbc`` files that ``extract_dxbc.py`` carves.
``--json`` emits the same records as a JSON list; ``--scan`` forces the fallback.
"""

import json
import os
import re
import struct
import sys

# The ShaderLibrary arrays, in declaration order, with the program type each one holds.
SHADER_ARRAYS = [
    ("VertexShaders", "vertex"),
    ("FragmentShaders", "fragment"),
    ("ComputeShaders", "compute"),
    ("GeometryShaders", "geometry"),
    ("HullShaders", "hull"),
    ("DomainShaders", "domain"),
]


def name_shaders(data: bytes) -> tuple[list[dict], str]:
    """Return the bundle's ``(shaders, mode)``, ordered by blob offset.

    ``mode`` is ``"name-table"`` or ``"backwards-scan"``. Each shader is a dict of ``index``,
    ``offset``, ``size``, ``type``, ``name``, and — from the name table only — ``name_hash`` and
    ``data_hash``.
    """
    try:
        shaders = _from_name_table(data)
        mode = "name-table"
    except BundleFormatError as e:
        print(f"warning: {e}; falling back to the backwards scan", file=sys.stderr)
        shaders = _from_backwards_scan(data)
        mode = "backwards-scan"
    shaders.sort(key=lambda s: s["offset"])
    for i, s in enumerate(shaders):
        s["index"] = i
    return shaders, mode


class BundleFormatError(Exception):
    """The bundle is not an ADF file holding a parseable ShaderLibrary instance."""


def _from_name_table(data: bytes) -> list[dict]:
    base = _shader_library_offset(data)

    # ShaderLibrary: Name*, BuildTime*, then six {ptr, count} arrays. Every pointer in the instance
    # is relative to the instance's own offset, not to the start of the file.
    header = _unpack(data, base, "<14Q", "ShaderLibrary header")

    shaders = []
    for i, (label, program_type) in enumerate(SHADER_ARRAYS):
        ptr, count = header[2 + i * 2], header[3 + i * 2]
        for k in range(count):
            record = base + ptr + k * 40
            # Shader: NameHash u32 @0, Name* @8, DataHash u32 @16, BinaryData* @24 + count @32
            # (an A[uint8] holding the DXBC container). The hashes sit in 8-byte-aligned slots.
            name_hash, name_ptr, data_hash, blob_ptr, blob_len = _unpack(
                data, record, "<I4xQI4xQQ", f"{label}[{k}]"
            )
            offset = base + blob_ptr
            if data[offset : offset + 4] != b"DXBC":
                raise BundleFormatError(f"{label}[{k}] does not point at a DXBC blob")
            shaders.append(
                {
                    "offset": offset,
                    "size": blob_len,
                    "type": program_type,
                    "name": _string(data, base + name_ptr) if name_ptr else "",
                    "name_hash": name_hash,
                    "data_hash": data_hash,
                }
            )
    if not shaders:
        raise BundleFormatError("the ShaderLibrary instance holds no shaders")
    return shaders


def _shader_library_offset(data: bytes) -> int:
    """Find the ShaderLibrary instance via the ADF header's instance and name tables."""
    if data[:4] != b" FDA":
        raise BundleFormatError("not an ADF file (bad magic)")
    instance_count, instance_offset = _unpack(data, 0x08, "<II", "ADF header")
    name_count, name_offset = _unpack(data, 0x20, "<II", "ADF header")

    # The name table is a run of u8 lengths followed by that many null-terminated strings.
    names, cursor = [], name_offset + name_count
    for _ in range(name_count):
        names.append(_string(data, cursor))
        cursor += len(names[-1]) + 1

    for i in range(instance_count):
        _, _, offset, _, name_index = _unpack(
            data, instance_offset + i * 24, "<IIIIQ", "ADF instance table"
        )
        if name_index < len(names) and names[name_index] == "ShaderLibrary":
            return offset
    raise BundleFormatError("no ShaderLibrary instance in the ADF instance table")


def _from_backwards_scan(data: bytes) -> list[dict]:
    """Recover names by walking back from each DXBC magic over the null-padded ASCII before it.

    A fallback only: for a fair number of blobs the preceding bytes are not the name, so this finds
    no name at all, or — worse — a stray printable byte that it reports as one. The program type
    does not come from the neighbourhood; it is read out of the blob's own shader chunk.
    """
    shaders = []
    for match in re.finditer(b"DXBC", data):
        offset = match.start()
        (size,) = struct.unpack_from("<I", data, offset + 0x18)
        if size <= 0 or offset + size > len(data):
            continue
        end = offset
        while end > 0 and data[end - 1] == 0:
            end -= 1
        start = end
        while start > 0 and 32 <= data[start - 1] < 127:
            start -= 1
        shaders.append(
            {
                "offset": offset,
                "size": size,
                "type": _program_type(data[offset : offset + size]),
                "name": data[start:end].decode("ascii", "replace"),
            }
        )
    return shaders


def _program_type(blob: bytes) -> str:
    """Read the program type out of a DXBC blob's shader chunk version dword."""
    types = ["fragment", "vertex", "geometry", "hull", "domain", "compute"]
    for magic in (b"SHEX", b"SHDR"):
        i = blob.find(magic)
        if i < 0:
            continue
        (version,) = struct.unpack_from("<I", blob, i + 8)
        kind = (version >> 16) & 0xFFFF
        return types[kind] if kind < len(types) else f"unknown({kind})"
    return "unknown"


def _string(data: bytes, offset: int) -> str:
    end = data.index(b"\0", offset)
    return data[offset:end].decode("ascii", "replace")


def _unpack(data: bytes, offset: int, layout: str, what: str) -> tuple:
    try:
        return struct.unpack_from(layout, data, offset)
    except struct.error as e:
        raise BundleFormatError(f"{what} at {offset:#x} runs past the end of the file") from e


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    flags = {a for a in sys.argv[1:] if a.startswith("--")}
    if len(args) != 1 or flags - {"--json", "--scan"}:
        sys.exit(f"usage: {sys.argv[0]} <bundle.shader_bundle> [--json] [--scan]")

    with open(os.path.expanduser(args[0]), "rb") as f:
        data = f.read()
    if "--scan" in flags:
        shaders, mode = _from_backwards_scan(data), "backwards-scan"
        shaders.sort(key=lambda s: s["offset"])
        for i, s in enumerate(shaders):
            s["index"] = i
    else:
        shaders, mode = name_shaders(data)

    if not shaders:
        sys.exit("no shaders found (is this a JC3 .shader_bundle?)")

    named = sum(1 for s in shaders if s["name"])
    print(f"{named}/{len(shaders)} named via the {mode}", file=sys.stderr)
    if "--json" in flags:
        json.dump(shaders, sys.stdout, indent=1)
        print()
    else:
        for s in shaders:
            print(f"{s['index']:04d} {s['offset']:08x} {s['type']:<8} {s['name']}")


if __name__ == "__main__":
    main()

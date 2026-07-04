#!/usr/bin/env python3
"""Live Windows-stack scanner for the game under Wine.

For every thread of PID: recovers the TEB (via one gdb batch call), the Windows stack
bounds, the Windows TID, and the Wine syscall frame (saved rip/rsp), then scans the
live band [saved rsp, StackBase) for return addresses inside the exe or the payload
DLL. Read-only (process_vm_readv); the process is never stopped except for gdb's brief
TEB enumeration.

Usage: python3 stackscan.py <pid>
"""

import ctypes
import ctypes.util
import re
import struct
import subprocess
import sys

EXE_RANGE = (0x140001000, 0x143500000)

libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)


class iovec(ctypes.Structure):
    _fields_ = [("base", ctypes.c_void_p), ("len", ctypes.c_size_t)]


def read_mem(pid: int, addr: int, size: int) -> bytes:
    buf = ctypes.create_string_buffer(size)
    local = iovec(ctypes.cast(buf, ctypes.c_void_p), size)
    remote = iovec(addr, size)
    n = libc.process_vm_readv(pid, ctypes.byref(local), 1, ctypes.byref(remote), 1, 0)
    return buf.raw[:n] if n > 0 else b""


def payload_range(pid: int):
    lo = hi = None
    for line in open(f"/proc/{pid}/maps"):
        if "jc3vrs_payload" in line:
            start, end = (int(x, 16) for x in line.split()[0].split("-"))
            lo = start if lo is None else min(lo, start)
            hi = end if hi is None else max(hi, end)
    return (lo, hi) if lo is not None else None


def teb_map(pid: int):
    """LWP -> TEB base, via one gdb batch enumeration."""
    out = subprocess.run(
        ["gdb", "-p", str(pid), "--batch", "-ex", "set pagination off",
         "-ex", 'thread apply all printf "GS %llx\\n", $gs_base'],
        capture_output=True, text=True,
    ).stdout
    pairs, lwp = {}, None
    for line in out.splitlines():
        m = re.search(r"LWP (\d+)", line)
        if m:
            lwp = int(m.group(1))
        m = re.match(r"GS ([0-9a-f]+)", line)
        if m and lwp:
            pairs[lwp] = int(m.group(1), 16)
            lwp = None
    return pairs


def syscall_frame(pid: int, teb: int, stack_lo: int, stack_hi: int):
    """The Wine syscall frame (saved Windows rip, rsp), found by shape."""
    for cand_off in (0x378,):  # Proton Experimental as of 2026-07.
        ptr = struct.unpack("<Q", read_mem(pid, teb + cand_off, 8) or b"\0" * 8)[0]
        frame = read_mem(pid, ptr, 0x90) if ptr else b""
        if len(frame) == 0x90:
            rip, rsp = struct.unpack_from("<Q", frame, 0x70)[0], struct.unpack_from("<Q", frame, 0x88)[0]
            if stack_lo <= rsp < stack_hi:
                return rip, rsp
    # Fallback: scan the TEB for any pointer passing the shape test.
    blob = read_mem(pid, teb, 0x1000)
    for off in range(0, len(blob) - 7, 8):
        ptr = struct.unpack_from("<Q", blob, off)[0]
        if not 0x10000 <= ptr <= 0x7FFFFFFFFFFF:
            continue
        frame = read_mem(pid, ptr, 0x90)
        if len(frame) == 0x90:
            rip, rsp = struct.unpack_from("<Q", frame, 0x70)[0], struct.unpack_from("<Q", frame, 0x88)[0]
            if stack_lo <= rsp < stack_hi and rip > 0x10000:
                return rip, rsp
    return None


def main():
    pid = int(sys.argv[1])
    payload = payload_range(pid)
    print(f"payload mapping: {tuple(hex(x) for x in payload) if payload else 'none'}")
    for lwp, teb in sorted(teb_map(pid).items()):
        head = read_mem(pid, teb, 0x50)
        if len(head) < 0x50:
            continue
        stack_hi, stack_lo = struct.unpack_from("<QQ", head, 8)
        win_tid = struct.unpack_from("<Q", head, 0x48)[0]
        if not (0 < stack_hi - stack_lo <= 16 * 1024 * 1024):
            continue
        frame = syscall_frame(pid, teb, stack_lo, stack_hi)
        if not frame:
            print(f"LWP {lwp} (WinTID {win_tid:#x}): running or no syscall frame")
            continue
        rip, rsp = frame
        lines = []
        stack = read_mem(pid, rsp, min(32768, stack_hi - rsp))
        for i in range(0, len(stack) - 7, 8):
            v = struct.unpack_from("<Q", stack, i)[0]
            if EXE_RANGE[0] <= v <= EXE_RANGE[1]:
                lines.append(f"  rsp+{i:#06x}: exe     {v:#x}")
            elif payload and payload[0] <= v <= payload[1]:
                lines.append(f"  rsp+{i:#06x}: payload {v:#x} (rva {v - payload[0]:#x})")
            if len(lines) >= 20:
                break
        if lines:
            print(f"LWP {lwp} (WinTID {win_tid:#x}): saved rip={rip:#x} rsp={rsp:#x}")
            print("\n".join(lines))


if __name__ == "__main__":
    main()

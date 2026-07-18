#!/usr/bin/env python3
"""Catch and dump a hard crash of the game under Wine, via gdb.

Wine fires SIGUSR1 (and realtime signals) constantly for its own thread scheduling.
A naive `handle SIGSEGV stop` catcher drowns in them, and gdb itself hits an internal
`get_thread_regcache` assertion when the game's threads start exiting mid-crash -- so a
live catch that does not silence wine's signals reliably captures nothing.

This attaches, tells gdb to ignore every wine signal, stops only on a genuine
SIGSEGV/SIGABRT, and then dumps the registers, the faulting instructions, a backtrace,
and a stack scan -- tagging any address that falls in the exe (engine) or the payload
DLL band, since a dangling detour (e.g. a COM-vtable hook left enabled at eject) jumps
into the former payload band.

Auto-resolves the JustCause3.exe PID (the one with jc3vrs_payload mapped) and the
payload/exe address bands from /proc/<pid>/maps. Override the PID with JC3_PID=<pid>.

Usage:
    gdb --batch -x catch_crash.py            # attach, then trigger the crash
    JC3_PID=12345 gdb --batch -x catch_crash.py

Afterwards, symbolize a payload RVA with
    llvm-symbolizer --obj=target/x86_64-pc-windows-msvc/debug/jc3vrs_payload.dll <0x180000000 + RVA>
and an exe address against $JC3_RELEASE_IDB (the exe loads at its preferred base, 1:1).
"""

import os
import subprocess

import gdb

WINE_SIGNALS = [
    "SIGUSR1", "SIGUSR2", "SIGTRAP", "SIGPIPE", "SIGCHLD",
    "SIG32", "SIG33", "SIG34", "SIG35", "SIG36", "SIG37", "SIG38",
]


def find_pid():
    if os.environ.get("JC3_PID"):
        return int(os.environ["JC3_PID"])
    out = subprocess.run(
        ["pgrep", "-f", "JustCause3.exe"], capture_output=True, text=True
    ).stdout
    for pid in out.split():
        try:
            if "jc3vrs_payload" in open("/proc/%s/maps" % pid).read():
                return int(pid)
        except OSError:
            pass
    return None


def module_band(pid, needle):
    lo = hi = None
    for line in open("/proc/%d/maps" % pid):
        if needle in line:
            a, b = (int(x, 16) for x in line.split()[0].split("-"))
            lo = a if lo is None else min(lo, a)
            hi = b if hi is None else max(hi, b)
    return (lo, hi)


pid = find_pid()
if pid is None:
    print("catch_crash: no JustCause3.exe with jc3vrs_payload mapped -- is the game running?")
    raise SystemExit(1)

payload = module_band(pid, "jc3vrs_payload")
exe = module_band(pid, "JustCause3.exe")

gdb.execute("set pagination off")
gdb.execute("set confirm off")
for sig in WINE_SIGNALS:
    try:
        gdb.execute("handle %s nostop noprint pass" % sig)
    except gdb.error:
        pass
gdb.execute("handle SIGSEGV stop noprint nopass")
gdb.execute("handle SIGABRT stop noprint nopass")
gdb.execute("attach %d" % pid)

print(
    "=== ATTACHED pid=%d payload=0x%x-0x%x; trigger the crash now ==="
    % (pid, payload[0] or 0, payload[1] or 0),
    flush=True,
)

# The first real SIGSEGV/SIGABRT stops us; dump and exit (do not loop -- looping past a
# fatal fault races the thread-exit gdb assertion this catcher exists to avoid).
try:
    gdb.execute("continue")
    rip = int(gdb.parse_and_eval("(unsigned long long)$pc"))
    print("\n=== FAULT rip=0x%x ===" % rip, flush=True)
    if exe[0] and exe[0] <= rip < exe[1]:
        print("--> in EXE (engine) at 0x%x" % rip, flush=True)
    elif payload[0] and payload[0] <= rip < payload[1]:
        print("--> in PAYLOAD (RVA 0x%x)" % (rip - payload[0]), flush=True)
    else:
        print("--> outside exe/payload (system lib, or a dangling jump target)", flush=True)
    gdb.execute("info registers rip rsp rbp rax rbx rcx rdx rdi rsi r8 r9 r10 r11 r12 r13")
    gdb.execute("x/8i $pc")
    gdb.execute("bt 24")
    print("--- stack (return addresses) ---", flush=True)
    gdb.execute("x/48a $rsp")
except gdb.error as e:
    print("catch_crash: %s (process may have exited before the fault was caught)" % e, flush=True)

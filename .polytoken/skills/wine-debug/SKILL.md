---
description: Live-diagnose the running (or hung) game process under Proton/Wine from native Linux tooling — threads, stacks, locks, and memory. Use when the game locks up, freezes, spins, or misbehaves and logs alone do not explain it.
polytoken:
  tags: [debugging, jc3]
---

# Debugging the running game under Wine

The Windows game process can be fully diagnosed from native Linux tooling — no
Windows debugger, no reproduction, and (mostly) without stopping the process.
The pillars:

- **ptrace works** on the Wine process, so gdb and `process_vm_readv` both
  function.
- **The exe loads at its preferred base** (`0x140000000`), so runtime addresses
  map 1:1 onto the release IDB.
- **Wine parks blocked threads in visible syscalls**, and its thread state
  (TEB, syscall frames) is reachable from outside.

Read the payload log first (`target/x86_64-pc-windows-msvc/debug/jc3vrs.log`
next to the built DLL); it says what the mod last did. Debug second.

## Resolve context at session start

Paths come from the environment (direnv loads `.env`); addresses come from the
pyxis definitions. Do not hardcode either.

```sh
echo "IDB=$JC3_RELEASE_IDB"   # release IDB for symbolizing exe addresses
pgrep -fa "JustCause3"        # the bare JustCause3.exe line is the game PID
grep jc3vrs_payload /proc/<pid>/maps | head -2   # payload mapping range
```

For any game address you need — singletons, field offsets, vtable slots,
function addresses — **consult the pyxis definitions before reading raw
memory**: grep `jc3gi/pyxis-defs/projects/JustCause3/Steam/20206564/` for the
type or field (`#[singleton(...)]`, `#[address(...)]`, `#[index(...)]`), or the
generated constants in `jc3gi/src` (`*_ADDRESS`). They are the project's ground
truth and carry the documentation; a raw address in a debugging session that
turns out to matter should be promoted into the defs afterwards.

## Triage: who is doing what

```sh
for t in /proc/<pid>/task/*/; do
  s=$(awk '{print $3}' "$t/stat"); [ "$s" = R ] && echo "$(basename $t) $(cat $t/comm)"
done
```

- One R-state thread during a hang → a busy-wait spin; everyone else is parked
  behind it. Sample the spinner first.
- Zero R-state threads → a pure wait cycle (locks/events). Go to the lock-state
  and stack-walk steps.

## Spinning threads: sample the instruction pointer

```sh
for i in 1 2 3; do gdb -p <tid> --batch -ex "info registers rip" 2>/dev/null | grep rip; sleep 0.3; done
```

A stable RIP inside the exe names the spin loop: `lookup_funcs` on the address
in the IDB, then `disasm` to read the loop (typically a flag poll — note which
address it polls; reading that flag's owner object tells you what never got
signalled). gdb's `bt` also yields the innermost return address (frame #1)
even though it cannot unwind Windows frames beyond that.

## Parked threads: recover the live Windows stack

The SP visible in `/proc/<tid>/syscall` and plain gdb is Wine's *syscall
dispatcher* stack, not the Windows stack. Recover the Windows state via the
TEB:

1. **LWP → TEB**: one batch call —
   `gdb -p <pid> --batch -ex "set pagination off" -ex "thread apply all printf \"GS %llx\\n\", \$gs_base"`.
   The `Thread ... (LWP n)` headers pair with the `GS` lines.
2. **TEB fields**: `+0x08` StackBase, `+0x10` StackLimit, `+0x48` the Windows
   TID (ClientId) — the TID↔LWP map is essential for interpreting lock owners.
3. **The Wine syscall frame** holds the saved Windows registers. Its pointer is
   at `TEB+0x378` on current Proton Experimental; since the offset is a Wine
   internal, verify by shape — the frame's `+0x70` (rip) must be code and
   `+0x88` (rsp) must fall inside [StackLimit, StackBase). If the offset moved
   with a Proton update, scan the TEB's first 0x1000 bytes for a pointer that
   passes the shape test.
4. **Scan qwords from the saved rsp upward.** Values inside the exe or payload
   mappings are the live return-address chain, innermost first.

`scripts/stackscan.py <pid>` does all four steps for every thread and tags each
hit exe/payload (with the payload RVA). It only uses `process_vm_readv`, so it
never stops the game.

**Stale-frame trap**: only the band [saved rsp, StackBase) is live. Anything
below the saved rsp — and any whole-stack-region scan — contains dead frames
from earlier calls, including *earlier calls of the same function you are
interested in*. Full-region scans are for triaging which threads are worth a
proper walk, never for reading a call chain.

## Symbolize

- **Exe addresses** → the IDA MCP on `$JC3_RELEASE_IDB`: `lookup_funcs` for
  names, `decompile`/`disasm` for bodies. The symbol dump
  (`$JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP`) gives readable reference bodies for
  whatever you find — see the jc3-reverse-engineering skill for the
  dump-to-release discipline.
- **Payload addresses** → RVA = address − payload mapping base (from
  `/proc/<pid>/maps`), then
  `llvm-symbolizer --obj=target/x86_64-pc-windows-msvc/debug/jc3vrs_payload.dll <0x180000000 + RVA>`
  (`0x180000000` is the DLL's ImageBase; confirm with `objdump -x` if in
  doubt). Output includes file:line into `payload/src`.

## Read game state without stopping the process

`process_vm_readv` (helper at the top of `scripts/stackscan.py`) reads any
mapped address live. The high-value reads during a deadlock or misbehaviour:

- **Lock owners.** A `CRITICAL_SECTION` is `LockCount` at `+0x08`,
  `RecursionCount` at `+0x0C`, `OwningThread` at `+0x10` (a *Windows* TID — map
  it to an LWP via the TEB ClientId). "Who holds this lock" plus "what is that
  thread's live stack" resolves most hangs outright.
- **Singletons and fields.** Resolve the singleton address and field offsets
  from the pyxis defs, then read the live values (state flags, thread-id
  fields, sync flags) to test hypotheses directly instead of guessing.
- **Vtables.** Read the object's first qword, then the slot pointers, and
  `lookup_funcs` each one. Comparing the live slot targets against what the
  bindings *think* they are calling catches wrong-slot bindings immediately —
  cheap to do whenever a vtable call is on the suspect list.

## Ground rules

- Everything here is read-only. Never `kill` game processes without asking —
  a hung process is a debugging asset, and killing it destroys the evidence.
- gdb attach/detach briefly pauses the process; prefer `process_vm_readv` for
  repeated reads, and batch gdb work into single invocations.
- Findings that name engine structures (field meanings, lock semantics, vtable
  slots) belong in the pyxis defs afterwards, per the RE skill's conventions.

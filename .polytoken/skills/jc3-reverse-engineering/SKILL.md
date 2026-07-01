---
description: Reverse-engineer Just Cause 3 against the release IDB and a symbol-bearing release-build dump, and capture findings as pyxis definitions.
polytoken:
  tags: [reverse-engineering, jc3]
---

# Reverse-engineering Just Cause 3

You are reverse-engineering the Windows DX11 release build of Just Cause 3
(Steam app 20206564). Two sources are available, and findings are captured as
[pyxis](https://github.com/ferrobrew/pyxis) definitions in this repo.

## The two sources

| Source | What it is | Role |
|---|---|---|
| **Release IDB** | `JustCause3.exe.i64` — the IDA database for the *release* build you are modding. | **Ground truth.** Addresses, sizes, and layouts you record come from here. |
| **Symbol dump** | A folder of decompiler output from a *2016 release build* of the game (FINAL/`-O2`/`-Z7`, shipped unprotected), with full symbols. | **Reference / locator.** Use it to find and name things, then verify against the release IDB. |

The dump is **not** a 1:1 map of the release build you are modding. Treat it as
a hint source, never as the source of record. See "Dump-to-release caveats"
below.

## Resolve the paths at session start

The IDB and dump paths are provided through environment variables (loaded by
direnv from `.env`). They are intentionally not hardcoded here so the skill stays
portable. Resolve them once at the start of an RE session:

```sh
echo "IDB=$JC3_RELEASE_IDB"; echo "DUMP=$JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP"
```

Both must be non-empty before you proceed. If either is empty, direnv has not
loaded `.env`; tell the user and stop. Otherwise remember the resolved values
for the rest of the session and use them directly in tool calls.

- `$JC3_RELEASE_IDB` — the `.i64` path. Pass it to `idb_open`.
- `$JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP` — the dump directory root.

## Open the IDB with an extended keepalive

The IDA MCP's default call timeout and idle TTL are too short for extended RE
sessions. When you open the IDB, keep the worker alive longer, and pass a
generous per-call timeout on analytical calls:

- `idb_open`: set `idle_ttl_sec` high (e.g. `3600`) so the headless worker does
  not self-exit mid-session. Use `mode: "prefer_headless"`.
- Every analytical call (`decompile`, `analyze_function`, `analyze_batch`,
  `disasm`, `survey_binary`, `callgraph`, `trace_data_flow`, …) takes a
  `timeout_seconds` argument. Pass a large value (e.g. `300` or more) for these;
  the default is often too short for large functions.
- Check `server_health` if a call stalls; reopen with `idb_open` if the session
  has gone away (it returns the existing session if still open).

Start every session with `survey_binary` on the open IDB to orient yourself.

## The symbol dump

Layout (root = `$JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP`):

- `*.h` — type/struct/union definitions (e.g. `AabbAndLayer.h`).
- `*.cpp` — function definitions. Each function starts with a header comment
  `//----- (00000001xxxxxxxx) -----` giving the **dump-build** address, then
  the decompiled body with full symbol names and class qualifiers
  (e.g. `GraphicsEngine::Draw`).
- `*$<hash>.h` — anonymous unions/structs keyed by an IDA hash.
- `_A0x<addr>/` and `A0x<addr>` — anonymous entries grouped by dump-build
  address.
- Named subdirectories (e.g. `NGSONodes/`) namespace grouped items.

How to use it:

- To find a symbol by name, `grep -rl "<SymbolName>" "$JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP"`
  (or `rg`). The `.cpp` files give you signature and structure; the `.h` files
  give you the type layout.
- The address in a dump `.cpp` header is a **dump-build** address. It will
  **not** match the release IDB. Use the symbol name and the function's
  *structure* (string refs, call sequence, constants, vtable shape) to locate
  the corresponding release function in the IDB, then record the **release**
  address from the IDB.
- Use `find_regex` / `search_text` / strings in the IDB to anchor a release
  function once you know what to look for from the dump.

## Build provenance (what the dump actually is)

Established from the accompanying PDB metadata: the symbol dump is a
**2016 optimized release build** (`FINAL`/`NDEBUG`/`-O2`, MSVC 2010), shipped
*unprotected* with symbols embedded per-object (`-Z7`). It is not an
unoptimized debug build. RTTI is disabled (`-GR-`), so use vtable shape and
call structure — not dynamic-cast artifacts — when reasoning about types.

## Dump-to-release caveats

This is the core discipline of the task. Both the dump and the release IDB are
optimized release builds — so the classic debug-vs-release gap (extra
instrumentation fields, unoptimized code shape) largely does not apply. They
differ in ways that still bite:

1. **Inlining and codegen drift.** The dump and the release IDB are separate
   compilations, not byte-identical builds. A function inlined in one may be a
   discrete call in the other. Match on *semantics* (strings, constants, call
   targets, vtable indices, structural shape), not on instruction-level
   patterns.
2. **Stripping and protection.** The release retail binary is stripped and may
   have protection that the unprotected dump does not. Symbols, string
   references, and layout hints present in the dump may be partially or wholly
   absent in the release IDB — the IDB's own analysis must reconstruct them.
3. **Struct layout can drift.** Despite both being release builds, struct
   sizes/offsets can diverge. Confirm offsets and sizes against the release IDB
   before recording them.

Operating rule: **the dump tells you what to look for and what to call it; the
IDB tells you where it actually is and how big it really is in the release
binary.** Always confirm offsets, sizes, and addresses against the release IDB
before recording them. When a dump struct has a field you cannot find in the
release IDB, suspect a branch/protection difference and verify by
re-decompiling the release type's consumers.

## Capture findings as pyxis definitions

RE findings are recorded as pyxis definitions — never edit the generated
`jc3gi/src` directly; edit the `.pyxis` source.

### Where

`jc3gi/pyxis-defs/projects/JustCause3/Steam/20206564/` — one `.pyxis` file per
module; folders nest modules; a folder that needs its own items gets a
`mod.pyxis`. The build script (`jc3gi/build.rs`) regenerates `jc3gi/src` from
these. `pyxis.toml` sets `pointer_size = 8` (x86_64).

### Conventions (from `jc3gi/pyxis-defs/CONTRIBUTING.md`)

- **Strip engine type prefixes.** `SVector3` → `Vector3`, `CGameObject` →
  `GameObject`. Keep a prefix only to avoid a collision.
- **Addresses.** `#[address(0x...)]` on struct fields (offset within the type)
  and on functions (release-build RVA). Use the release IDB address, not the
  dump-build address. Underscore-separate hex groups for readability:
  `0x140_0F4_170`.
- **Type shape.** `#[singleton(0x142_E2B_6F0)]` for singletons (absolute
  release address), `#[size(0x70)]` / `#[min_size(0x1F10)]` for sized types,
  `#[align(8)]`, `#[copyable]` / `#[cloneable]` / `#[defaultable]`.
- **Inheritance.** `#[base]` on a region for composition-based inheritance.
- **Vtables.** `__vftable: u64` field; `#[index(n)]` on vftable entries.
- **Opaque backend types.** `extern type` with `#[size(..)]`, `#[align(..)]`,
  `rust_name = "..."`, `cpp_header = "..."`, `cpp_name = "::..."`.
- **Imports.** `use graphics_engine::{device::Device, texture::Texture};` —
  same syntax as Rust.
- **Docs.** `///` doc comments become the docs. Explain *why* and the non-obvious
  *what*; follow the repo's documentation conventions (periods, sentence case,
  Oxford comma, no narrative comments in function bodies).
- **Use-case-neutral.** pyxis-defs is a shared, multi-game RE database that
  describes the engine as it is, independent of any consumer. Every definition and
  doc comment must be use-case-neutral: describe *engine* behaviour only, and never
  the downstream project's use of it. No references to the mod, to stereo / VR
  rendering, "per eye", between-eye snapshot/restore, or any consumer-specific
  framing — say what a field/function *is* and how the engine itself uses it, and
  leave why a consumer cares to the consumer's own code. When promoting a raw offset
  from consumer code into a definition, strip the consumer rationale from the doc.

When you record a function or field, add a `///` doc comment capturing what you
established: what the function does, the semantic meaning of a field, or the
lifecycle of a state machine. Cross-reference related items with
`[`Name`](Module::Name)` links.

### Verify before finishing

```sh
pyxis fmt          # canonical formatting (pyxis fmt --check to verify without rewriting)
```

Run from `jc3gi/pyxis-defs/`. There is no need to run the pyxis build or
`--check-builds` separately: the generated Rust is compile-checked when you
build `jc3gi` as part of the workspace.

### Submodule workflow

`pyxis-defs` is a git submodule with CI that auto-updates its docs.

1. **Before editing:** `cd jc3gi/pyxis-defs && git pull` so you are not working
   against a stale commit.
2. Commit and **push** `pyxis-defs` first — the submodule pointer must update
   before the main repo.
3. Wait for the docs CI commit, then pull again.
4. The main-repo commit includes both the submodule pointer bump and the
   regenerated `jc3gi/src` bindings.

This is **only** for a pyxis-defs change that is part of a larger piece of work.
Do **not** auto-commit `pyxis-defs` after every RE finding. Stage and verify
your `.pyxis` edits locally, and only run the push/submodule-pointer workflow
when you are ready to move on to whatever consumes the new definitions. Until
then, leave the changes uncommitted in the working tree.

## Workflow summary for an RE task

1. Resolve `$JC3_RELEASE_IDB` and `$JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP`.
2. `idb_open` the IDB with `idle_ttl_sec: 3600`, `mode: "prefer_headless"`.
3. `survey_binary` to orient.
4. Use the dump (`grep`/`rg` for symbols) to find candidate names and structure.
5. Locate the release counterpart in the IDB (strings, constants, call shape,
   vtable index). Confirm it is *not* an inlined/branch-only artifact.
6. Record the **release** addresses/sizes/offsets in the right `.pyxis` file
   under `20206564/`, with a `///` doc comment capturing what you established.
7. `pyxis fmt` + `python build.py --no-install --check-builds rust`.
8. Follow the submodule push-then-main-repo workflow when committing.

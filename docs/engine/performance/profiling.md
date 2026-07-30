# Performance-profiling machinery in the release build

Reverse-engineered from the 2026 Denuvo-less Steam build of Just Cause 3 (Apex engine),
against the release IDB, with the symbol-bearing 2016 release-build dump as the locator.
This is a recon survey: what timing machinery the Apex engine ships, which parts are still
*live* (constructed and called) in retail versus compiled out, and the cheapest way for the
mod to get per-pass CPU and GPU timings for VR (stereo rendering has to hit HMD frame rate,
so we want the engine's own timing data rather than building instrumentation from scratch).

All function addresses are RVAs from this build's `.text` and are build-specific. Struct
layouts (offsets, singletons) are byte-stable across the Denuvo / Denuvoless / debug builds,
as elsewhere in `docs/`. Where a claim rests on the dump plus indirect release evidence
rather than a decompiled release function, it is flagged **[UNVERIFIED]**.

## TL;DR — live versus dead, and the recommendation

| Machinery | Status in retail | What it gives you |
|---|---|---|
| `CpuProfiler` (`g_CpuProfiler`, 15 fixed frame phases) | **Dead** — `Update` compiled to a nullsub, no phase brackets write the counters; consumers read zeros (§1.1) | — |
| `Graphics::BeginScopeMarker` / `EndScopeMarker` | **Compiled out, but ~72 call sites and their name strings survive** (§1.5) | Named begin/end scope pairs across the whole draw thread, restorable by detouring two nullsubs |
| `SProfiler` scope primitives (`ProfilerThreadEnter`/`Leave`/`Add`/`AddBudget`) | **Live**, callable | Hierarchical per-thread scope timing with named entries and budgets |
| `Graphics::*TimeStamp*` / `*Disjoint*` query wrappers | **Live**, thin D3D11 wrappers | Raw GPU timestamp + frequency queries |
| `CGPUBufferedQuery` (ring-buffered GPU queries) | **Live** | N-frame-latency timestamp / occlusion / frequency read-back |
| `CRenderPassGpuTimingQuery` | **Live but wired to one pass** (particles) | GPU ms for the particle pass only, drives adaptive quality |
| `CProfilerUtil` singleton (budget store) | **Partially live** — object constructed, `LoadBudget` called; per-frame `Update`/`Render`/`PreUpdate` **stripped** | Loads a budget table; no on-screen display in retail |
| `CFrameProfiler` (on-screen frame graph) | **Dead** — no methods in retail | — |
| Dev console / cvar commands (`debug_DumpProfilerTrace`, `debug_fprof`, `frame_profiler`) and the `Dev\|Performance\|*` debug menu | **Dead** — strings and handlers stripped | — |
| `CSteeringFrameProfiler` (AI-steering frame graph) | **Live but niche** | Steering-solver timing, unrelated to the render pipeline |

**Recommendation (revised after the follow-up dig).** For **CPU**, there is nothing to read —
`g_CpuProfiler` is dead (§1.1) — so bracket the phase functions mod-side: `CGame::Update` /
`UpdateGame` / `UpdateRender` / `Draw` and `WaitForCPUDrawToFinish` on the main thread, and the
draw seams on the draw thread. For **named engine scopes**, detour the two scope-marker nullsubs
(§1.5): ~72 surviving call sites plus the per-render-block-type runs light up at once. For
**per-pass GPU**, the engine's own per-pass timer only covers particles and the `CGPUProfiler`
read-back is stripped (§2.2), so build a **small mod-side GPU-timestamp layer** that brackets
the existing Draw seams (`PreDraw` / `DrawGBuffer` / `Draw` / `DrawPosteffects` / `PostDraw`,
`rendering.md` §1.4) with the engine's *already-shipped* `Graphics::SetTimeStampQuery` +
disjoint wrappers on the immediate context. That reuses the engine's D3D11 query plumbing (no
new device setup), attributes GPU time to the coarse pass groups already named in
`rendering.md`, and is far cheaper and more robust than trying to revive the stripped
`CFrameProfiler` / `CProfilerUtil` / `CGPUProfiler` display paths. Detail below.

---

## 1. CPU side

### 1.1 `CpuProfiler` — the fixed 15-phase frame breakdown (dead in retail)

> **Correction (follow-up dig).** The earlier assessment below overstated liveness. In retail,
> `g_CpuProfiler` sits at **`0x142E5ADF0`** (pinned via the `this` pointer passed to
> `GetScopeName` in `CBorkReport::WriteMetrics`, and the direct `m_Time` reads in
> `CBorkReport::UpdateMetricsData`), but the machinery is **dead**: `CpuProfiler::Update` is
> compiled to an empty function (`0x140062490`, still called at the top of `CGame::Update`,
> `0x14096051b`), and no surviving code writes `m_Counters` — the phase brackets that
> `CGame::UpdateCPUProfiler` and the game states placed in the dump build are compiled out. The
> only interior reference is `CGraphicsEngine::DispatchDraw` (`0x1400f3a30`) forwarding the
> never-incremented `m_Index` to the draw task. `CBorkReport` still serialises the per-phase
> values into telemetry, but they are all zero. **There is nothing to read**; per-phase CPU
> timing has to be rebuilt mod-side by bracketing the phase functions
> (`CGame::Update` / `UpdateGame` (`0x1409521f0`) / `UpdateRender` / `Draw`,
> `CGraphicsEngine::WaitForCPUDrawToFinish`, and the draw-thread seams). The struct layout and
> addresses below remain correct and are captured in `cpu_profiler.pyxis`.

The engine keeps a single global `CpuProfiler` (`g_CpuProfiler` in the dump) that accumulates
wall-clock time for a fixed set of frame phases, the `CpuScopeId` enum:

```
CPU_SCOPE_ID_FRAME, DRAW, UPDATE_ALL, RENDER_ALL, PREUPDATE,
PRESIM_SYSTEMS, PRESIM_OBJECTS, PHYSICS_UPDATE, POSTSIM_UPDATE, POSTSIM_SYSTEMS,
WAIT_FRAME, RENDER_UPDATE, RENDER_SYSTEMS, WAIT_FLIP, WAIT_UI   (COUNT = 15)
```

The struct (from the dump, byte-stable) is compact:

```
struct CpuProfiler {
  float          m_Time[15];        // last frame's ms per phase
  float          m_Peak[15];        // 30-frame peak per phase
  int            m_Index;           // ring position (% NUM_FRAMES == 3)
  unsigned __int64 m_Counters[15][3][2];  // QPC begin/end, triple-buffered
  float          m_LocalPeak[15];
};
```

`CpuProfiler::Update` converts the QPC counter deltas to milliseconds
(`1000.0 / QueryPerformanceFrequency`) into `m_Time[]` and rolls a 30-frame peak into
`m_Peak[]`. `CGame` calls `CpuProfiler::Update(&g_CpuProfiler)` once per frame. The phase
brackets are written by `CGame::UpdateCPUProfiler` (dump `CGame.cpp:296`), which wraps the
work in `ProfilerThreadEnter` / `ProfilerThreadAdd` / `ProfilerThreadLeave` (see §1.2). The
release binary retains the scope-name accessor and the QPC accumulator:

- `CpuProfiler::GetScopeName` — `0x1400624a0`; returns `off_142D3A150[id]`, the scope-name
  string table (a 15-entry `const char*[]` at `0x142D3A150`).

Liveness is anchored by its **consumers**: `CBorkReport::WriteMetrics` (`0x140878330`) and
`CBorkReport::WriteTotalEntry` (`0x1408a30f0`) — Avalanche's crash/telemetry report — call
`GetScopeName` to serialise the per-phase CPU times into the report. So the CPU profiler is
not just present, it is *consumed* every crash/telemetry cycle. **[UNVERIFIED]** the release
RVA of `CGame::UpdateCPUProfiler` and the absolute address of `g_CpuProfiler` were not pinned
down this round; both are reachable from `CGame::Update` and from the `GetScopeName`
call sites, and the struct layout above is byte-stable.

**How the mod reads it.** Once `g_CpuProfiler`'s address is recorded (follow the QPC
accumulator out of `CGame::Update`, or the `off_142D3A150` consumers), reading
`m_Time[CPU_SCOPE_ID_DRAW]` etc. is a plain field read — no hooks, no enabling flag. This is
the single cheapest CPU-timing win.

### 1.2 `SProfiler` scope primitives (live, callable)

Underneath `CpuProfiler` is the general Apex scope profiler `SProfiler` — a per-thread,
hierarchical enter/leave timer with named entries (`SProfilerEntryData { volatile int m_ID;
float m_Budget; const char* m_Name; }`) that are lazily allocated and registered per module.
The free-function entry points survive in retail and are directly callable:

| Function | RVA | Role |
|---|---|---|
| `ProfilerThreadEnter(entryId, startQPC)` | `0x1404ccbe0` | push a scope on the current thread |
| `ProfilerThreadLeave(entryId, endQPC)` | `0x1403031f0` | pop the scope |
| `ProfilerThreadAdd(entryId, deltaQPC)` | `0x141e6a9a0` | add a raw duration to an entry |
| `ProfilerThreadAddBudget(entryId, deltaQPC, budget)` | `0x140646ef0` | add with a budget comparison |
| `ProfilerGetProfilerThreadID()` | `0x141e6bdf0` | current thread's profiler id |
| `ProfilerInitThread(name)` | `0x140f1ae60` | register a thread with the profiler |

(ICF folded a second `ProfilerThreadLeave` at `0x141e654f0` and a second
`ProfilerThreadAddBudget` at `0x14005f780`; they are identical bodies.)

Registered scope names survive as `SProfilerEntryData` records in `.data` — e.g. a record at
`0x142D6ED70` whose `m_Name` points at `"CGame::UpdateCPUProfiler"` (`0x1423EFE10`), and one
at `0x142D6FE10` for `"ProfilerInitThread"`. These are the static per-scope registration
records the enter/leave macros reference.

**How the mod could use it.** Allocate an entry id (via the profiler's allocate-entry hook,
or reuse an existing one) and bracket any code with `ProfilerThreadEnter` / `ProfilerThreadLeave`.
This would let the mod attribute CPU time to its *own* scopes (e.g. per-eye dispatch, VR submit)
that then show up alongside the engine's phases. But note the *read-back*/display side is
stripped (§1.3), so the mod would also have to read the raw entry table itself. For coarse
per-pass CPU timing, wrapping the `DrawRenderPassRange` seams with `ProfilerThreadEnter/Leave`
works, but a plain QPC read around those same seams is simpler and avoids depending on the
global profiler state.

### 1.3 `CProfilerUtil` — budget store lives, display is stripped

`CProfilerUtil` is a `Base::CSingle<CProfilerUtil>` singleton that, in the *dump* build, owned
the whole developer-facing performance UI: a budget table (`SBudgetEntry m_Budget[64]`,
loaded from `settings/budgetscopes.txt`), an on-screen budget/hitch display, a captured-trace
writer to `budget_violations.json`, a pointer to a `CFrameProfiler`, and a
`CDumpProfilerTraceCmd` console command. In the dump, `CGame` drove it every frame:
`CProfilerUtil::PreUpdate` / `Update` / `Render` / `SetDisplayBudgetScopes`.

In **retail, most of that is gone**. What survives:

- `CProfilerUtil::LoadBudget` — `0x140558b50`, and it is **still called**, from
  `CResourceLoaderRuntimeContainerHandler_CreateObjectsAsync` (`0x14055c160`) — so a budget
  table still loads at runtime.
- `~CProfilerUtil` — `0x1409527a0`, called from `~CGame` (`0x140967600`) — so the singleton is
  still constructed and torn down.
- `Base::CSingle<CProfilerUtil>::Release` — `0x140952a60`.

What is **stripped**: `PreUpdate`, `Update`, `Render`, `UpdateBudgetsOnScreen[QA]`,
`HandleUserInput`, `SetDisplayBudgetScopes`, `DoCapture`, `HitchCallback`, and the
`CDumpProfilerTraceCmd` handler — none appear in the release symbol table. The corroborating
string evidence: the dump's `"Dev|Performance|Main Thread Budgets"` /
`"Dev|Performance|Show Budget Scopes"` debug-menu paths, the `"debug_DumpProfilerTrace"` /
`"debug_fprof"` / `"frame_profiler"` console-command names, and `"settings/budgetscopes.txt"`
are **all absent** from the release IDB's string table (which *does* still contain
`"CGame::UpdateCPUProfiler"`, `"ProfilerInitThread"`, and the `PROFILER_*` input-action names,
so the strings are being identified — the missing ones are genuinely compiled out).

Net: the budget *data* still loads and budget comparisons via `ProfilerThreadAddBudget` can
still run, but there is **no interactive display, no console command, and no debug menu** in
retail. Reviving that path is not a cheap lever.

### 1.4 `CFrameProfiler` and the dev console — dead

`CFrameProfiler` is the classic on-screen frame-time graph (category rows, history buffer,
sort modes, an `m_AsyncGPUContext` GPU-timing lane). **No `CFrameProfiler` methods exist in
the release binary** — it is fully stripped. The developer console / cvar commands that would
toggle it are likewise gone (the command-name strings above are absent). Treat the on-screen
frame graph as unavailable; there is nothing to flip.

### 1.5 `Graphics::BeginScopeMarker` / `EndScopeMarker` — compiled out, call sites intact

The renderer is instrumented throughout with named scope markers:
`Graphics::BeginScopeMarker(HContext_t*, const char*)` opens a named scope on a graphics context
and `Graphics::EndScopeMarker(HContext_t*)` closes the innermost one, with an RAII wrapper
`Graphics::CScopeMarker` (ctor `0x140099990`, dtor `0x1400999C0`) calling the pair. In retail
both entry points are compiled to empty functions — **`0x141954D10`** (begin) and
**`0x141954D20`** (end) — but, unlike the `CFrameProfiler` display path, the *call sites and
their name strings survive*: ~72 sites covering the frame stages in
`CGraphicsEngine::HandleDrawThreadTask` ("Frame setup", "CopyEffectTextures", "Debug UI"), the
whole post-effect stack (SMAA/FXAA, depth of field, motion blur, lens flare, glare, tone-mapping
histogram), deferred lighting, SSAO, SSR, environment reflections, water, terrain restores, UI
and video off-screen rendering — and, most usefully, the per-render-block-type runs inside every
pass: `CRenderPass::ChangeRenderBlockType` (`0x140187310`) closes the previous type's scope and
opens the next named by the type's `GetTypeName()`, with `CRenderPass::DoDraw` closing the last
run. Detouring the two nullsubs therefore restores engine-wide named scope instrumentation on
the draw thread wholesale. Both markers and the timestamp wrappers are captured in
`graphics_engine.pyxis`.

### 1.6 `CSteeringFrameProfiler` — live but off-topic

One frame profiler *does* survive with live code: `CSteeringFrameProfiler`, the AI vehicle-steering
profiler. `LoadActionMap` (`0x140e847f0`) binds a `"frameprofiler"` input action map (the sole
code reference to that string), and `GetFrameProfilerInputs` (`0x140e98580`) /
`FrameProfilerOverridePlayerInputs` (`0x140df69d0`) / the ctor (`0x140e98200`) are all present.
It profiles the steering solver, not the render pipeline — not useful for VR frame-rate work,
noted only so it is not mistaken for the general frame profiler.

---

## 2. GPU side

### 2.1 `Graphics::*` timestamp and disjoint-query wrappers (live)

The engine ships thin, live wrappers over D3D11 timestamp queries. `CreateTimeStampQuery`
builds a `D3D11_QUERY_DESC` with `Query = 2` (`D3D11_QUERY_TIMESTAMP`) and calls the device's
`CreateQuery` (device vtable `+192`); the disjoint variant uses `Query = 3`
(`D3D11_QUERY_TIMESTAMP_DISJOINT`).

| Function | RVA |
|---|---|
| `Graphics::CreateTimeStampQuery` | `0x141955850` |
| `Graphics::DestroyTimeStampQuery` | `0x1419558a0` |
| `Graphics::SetTimeStampQuery` (records a timestamp on a context) | `0x1419558b0` |
| `Graphics::QueryTimeStamp` (blocks/reads the u64 tick) | `0x141955920` |
| `Graphics::CreateTimeStampDisjointQuery` | `0x1419559c0` |
| `Graphics::DestroyTimeStampDisjointQuery` | `0x141955a10` |
| `Graphics::BeginTimeStampDisjointQuery` | `0x141955a20` |
| `Graphics::EndTimeStampDisjointQuery` | `0x1419558B0` — ICF-folded with `SetTimeStampQuery` (both bodies are `ID3D11DeviceContext::End` under the context mutex); the symbol table's placement at `0x141954000` is an unrelated 3-byte stub, do not call it |
| `Graphics::QueryTimeStampFrequency` (reads freq + `Disjoint` flag) | `0x141955b00` |

These are the exact primitives a mod-side GPU timer wants: create timestamp/disjoint queries
on the `ID3D11Device` the mod already holds, `SetTimeStampQuery` around the work on the
immediate context, then `QueryTimeStamp` + `QueryTimeStampFrequency` a few frames later.
Reusing them (rather than calling `ID3D11DeviceContext` directly) keeps the mod coherent with
the engine's own query handles and error paths.

### 2.2 `CGPUBufferedQuery` — ring-buffered GPU queries (live)

`NGraphicsEngine::CGPUBufferedQuery` is the engine's general N-frame-latency query ring, built
on the wrappers above. Fully present in retail:

| Function | RVA |
|---|---|
| `Create` | `0x1400a0740` |
| `Destroy` | `0x1400a08c0` |
| `SetTimestamp` | `0x1400a04e0` |
| `SetFence` | `0x1400a0480` |
| `SetOcclusionBegin` / `SetOcclusionEnd` | `0x1400a0540` / `0x1400a05a0` |
| `SetFrequencyBegin` | `0x1400a05d0` |
| `GetOcclusionResult` | `0x1400a06c0` |
| `GetFrequencyResult` | `0x1400a0700` |

It holds `m_Timestamps[]` and `m_Frequency[]` arrays sized `m_NumBuffers * m_NumTimestamps`,
so a caller can record many timestamps per frame and read them back `m_NumBuffers` frames
later without a GPU stall. `rendering.md` §1.5 already notes a live "GPU-profiler frame-query
ring" at `engine + 5824`, advanced once per real frame in the `GraphicsEngine::Draw` prologue —
that is a `CGPUBufferedQuery` instance the engine already runs.

**Resolved (follow-up dig): the ring belongs to `NGraphicsEngine::CGPUProfiler`** — the engine's
full developer GPU profiler (per-scope GPU + CPU time, occlusion, draw-call counts; scope types
frame/render-pass/render-block/user; a rolling 10-frame capture mode; per-category results over
the 30 `ERenderPassCategory` groups). Its singleton sits at `0x142E5B668`. What survives in
retail: the constructor (`0x1400f3e60`), `Init` (`0x1401a1e20`, called from
`CGraphicsEngine::InitializeSystem`), `GetScopeResultCount` (`0x1400b7ef0`), and a frame-level
disjoint-query begin/end in `HandleDrawThreadTask`. What is stripped: the per-scope
`BeginScope`/`EndScope` call sites (the scope-marker nullsubs of §1.5 are their entry points),
the results processing, and the read-back — `GetScopeResult` (`0x1400a0f90`) is compiled to
`xor eax, eax; ret`, so `CBorkReport`'s per-category GPU metrics loop exits immediately. The
class cannot be revived by flipping state; treat it as a struct-layout reference only. (Several
release symbols in this area are ICF-misplaced — a body at `0x141954850` labelled `BeginScope`
is an unrelated vtable-dispatch wrapper, and one at `0x1401566d0` labelled `EndScope` is
vertex-buffer garbage collection; do not trust those names.)

### 2.3 `CRenderPassGpuTimingQuery` — live, but a single-pass adaptive-quality timer

There *is* a per-pass GPU timer in the render engine — but it is dedicated to **one** pass.
`NGraphicsEngine::CRenderPassGpuTimingQuery` triple/quad-buffers timestamp-start/end +
frequency queries and computes a rolling average (`m_ParticleGPUTimingHistory[10]`):

| Function | RVA |
|---|---|
| `Create` | `0x1400a0a20` |
| `Destroy` | `0x1400a0ac0` |
| `BeginGPUTimingQuery` | `0x1400a0b40` |
| `EndGPUTimingQuery` | `0x1400a0c70` |
| `GetAverageGpuTiming` | `0x1400a0cc0` |

The engine owns exactly one instance, `m_ParticlePassGpuTimingQuery`, on the `CRenderEngine`
singleton. It is driven from `CRenderEngine::BeginParticlePassGpuTimingQuery` (`0x140173960`),
which is called from `CRenderPass::DoDraw` (`0x1401ac7a0`, site `0x1401ac8b8`) and pairs with an
`EndGPUTimingQuery` at the tail of the particle draw. `GetAverageGpuTiming`'s result is compared
against ~4–5 ms thresholds to set `m_ParticlePassGpuIntense` — i.e. this is an **adaptive
particle-quality** system, not a general profiler. It measures only particles and its output
is a quality flag, not a readable per-pass table.

The class is a ready-made *template* for what a mod-side per-pass timer looks like (start/end
timestamps + disjoint frequency, ring-buffered, averaged), but it cannot be pointed at other
passes without new instances and new begin/end call sites.

---

## 3. Per-pass structure the mod can attach to

`rendering.md` §3 documents the pass system: a flat `ERenderPass` enum of ~180 `RP_*` values,
drawn by index range via `CRenderEngine::DrawRenderPassRange(ctx, setup, first, last)` over the
157-entry `m_RenderPasses[]` array. The coarse groups are already named and bracketed by
discrete calls in the `GraphicsEngine::Draw` prologue:

- `CRenderEngine::DrawGBuffer` — passes `0x2F..0x55` (`Draw+0x259`).
- `CRenderEngine::Draw` — passes `0x56..0x95` (lighting/SSR/reflection/main) (`Draw+0x35D`).
- `CRenderEngine::DrawPosteffects` — pass `0x96` (`Draw+0x3D2`).
- `CRenderEngine::PostDraw` — UI/debug/final copy (`Draw+0x486`).

There is **no live general per-pass GPU or CPU bookkeeping** to read (the only per-pass GPU
timer is the particle one in §2.3). But these four seams are exactly the granularity a VR
optimisation pass needs, and each is a single named function call — cheap to bracket with a
mod-side timestamp on both the CPU and GPU sides.

---

## 4. Recommendation for the VR mod

**CPU: bracket the phase functions mod-side — `g_CpuProfiler` is dead (§1.1).** The singleton
is pinned (`0x142E5ADF0`, captured in `cpu_profiler.pyxis`), but nothing writes it in retail,
so per-phase CPU timing has to be rebuilt by hooking the phase functions themselves:
`CGame::Update` (frame) / `UpdateGame` (sim tick) / `UpdateRender` / `Draw` and
`WaitForCPUDrawToFinish` on the main thread, `HandleDrawThreadTask` and the Draw seams (§3) on
the draw thread, and the scope-marker nullsubs (§1.5) for named engine scopes everywhere else.
`ProfilerThreadEnter/Leave` (`0x1404ccbe0` / `0x1403031f0`) remain live and callable, but with
the read-back side stripped a mod-side clock is simpler.

**GPU: build a small mod-side timestamp layer around the Draw seams — do not revive the
engine UI.** The engine's per-pass GPU timer covers only particles, and the `CFrameProfiler`/
`CProfilerUtil` display is stripped, so there is nothing to enable. Instead:

1. Create one disjoint query + a handful of timestamp queries per frame via the engine's own
   `Graphics::CreateTimeStampDisjointQuery` / `CreateTimeStampQuery` on the `ID3D11Device` the
   mod already holds.
2. On the **immediate context** (`Context::m_Context`, under `Context::m_Mutex` per
   `rendering.md` §"VR implications"), `BeginTimeStampDisjointQuery` at frame start,
   `SetTimeStampQuery` before/after each of `DrawGBuffer` / `Draw` / `DrawPosteffects` /
   `PostDraw`, and `EndTimeStampDisjointQuery` at frame end (its live body is the
   `SetTimeStampQuery` address, `0x1419558B0` — see §2.1's fold note).
3. Read back `QueryTimeStamp` + `QueryTimeStampFrequency` two–three frames later (or wrap the
   whole thing in a `CGPUBufferedQuery` from §2.2 to get the ring buffering for free).

This reuses shipped, live D3D11 query plumbing (no new device/query setup, no fight with the
stripped debug UI), attributes GPU time to the coarse pass groups already reverse-engineered in
`rendering.md`, and naturally extends to per-eye attribution by tagging each dispatch with
`STEREO_STATE.draw_index`. It is strictly less work and less risk than reconstructing the
`CRenderPassGpuTimingQuery` pattern for every pass or trying to bring back the dead frame graph.

**The existing ring is not a shortcut.** The `engine + 5824` `CGPUBufferedQuery` frame ring
(`rendering.md` §1.5, §2.2) belongs to the stripped `CGPUProfiler` (§2.2): retail only begins
and ends a frame-level disjoint query with it, the per-scope recording never runs, and the
read-back is stubbed. Issue mod-side queries instead.

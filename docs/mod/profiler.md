# The in-game profiler (issue #34)

The mod carries its own frame profiler because the engine's is dead in retail: the 15-phase
`CpuProfiler` is never written (`docs/engine/profiling.md` §1.1), the `CGPUProfiler` read-back is
stubbed (§2.2), and the on-screen frame graph is stripped (§1.4). What survives — the live D3D11
timestamp-query wrappers and the hookable frame-phase functions — is exactly enough to rebuild the
pipeline mod-side, which is what `payload/src/profiler/` does on top of
[puffin](https://github.com/EmbarkStudios/puffin).

Everything is compiled under the `profiler` cargo feature (default on). With the feature on but the
profiler idle (panel closed, no capture), each instrumented site costs a relaxed atomic load (plus
a few thread-local touches per dispatch for the lane labels and the type-run slot); with the
feature off, the hooks and scopes do not exist.

## Using it

- **Performance tab → "Profiler (issue #34)"**: a checkbox enables scope collection and shows
  puffin's live flame graph; a button captures ~5 s of frames.
- **F9** starts the same capture without the overlay (usable in-headset). Progress shows in the
  collapsible; the result is logged.
- Captures are written next to the payload DLL as `jc3vrs-profile-<timestamp>.json` in Chrome
  trace-event format — open in [ui.perfetto.dev](https://ui.perfetto.dev) (or `chrome://tracing`).
  Each puffin thread is a named lane ("game", "draw", "GPU"); timestamps are rebased to the
  capture start. Serialization runs on a background thread (a capture is tens of megabytes of
  JSON), so the game keeps rendering while the file is written.

## What is instrumented

CPU scopes, main thread: `CGame::Update` (the frame), `UpdateGame` (each sim tick),
`UpdateRender`, the per-eye dispatch loop (one `Dispatch` scope per `CGame::Draw`, tagged with the
eye and ordinal), and the post-Draw drain (`WaitForCPUDraw + drain`).

CPU scopes, draw thread: `RenderEngine::PreDraw` / `DrawGBuffer` / `Draw (scene)` /
`DrawPosteffects` / `PostDraw`, `DrawRenderPassRange` (with the pass range as data),
`RenderPass::DoDraw` (named per pass via the engine's own `GetRenderPassName` table), and — inside
each pass — one scope per render-block-type run, named by the type (`CRenderPass::
ChangeRenderBlockType` mirrors the engine's own compiled-out scope markers; see
`docs/engine/profiling.md` §1.5).

GPU lane: a synthetic "GPU" puffin thread built from the engine's own
`Graphics::CreateTimeStampQuery` / `SetTimeStampQuery` / disjoint-query wrappers. Each dispatch
wraps its seams (`PreDraw`, `DrawGBuffer`, `Draw (scene)`, `DrawPosteffects`, `PostDraw`) in
timestamp pairs under one disjoint query, and the resolved results are reported a few frames later
under a per-dispatch outer scope ("GPU eye 0" / "GPU eye 1" / "GPU far field"). GPU ticks are
mapped onto the CPU timeline via a CPU reference taken at the dispatch's start, with consecutive
dispatches serialized on the lane (the GPU executes them in order), so the lane aligns with — and
visibly trails — the CPU work that submitted it. Disjoint intervals are dropped rather than
reported. One known cosmetic effect: because a dispatch's GPU results land in the puffin frame
current at read-back time (~2-3 frames later), the live view's frame bars read wider than the true
frame time; the Chrome trace, being absolute-time, is unaffected. Trust the lane's durations, not
the frame bars.

## Structure

- `payload/src/profiler/mod.rs` — the switchboard: the per-frame `new_frame`, the enable state,
  and the dynamic-scope registry for engine-supplied names.
- `payload/src/profiler/gpu.rs` — the GPU timestamp ring and the puffin GPU-lane reporting.
- `payload/src/profiler/capture.rs` — the 5 s capture state machine (a puffin frame sink).
- `payload/src/profiler/chrome_trace.rs` — puffin frames → Chrome trace-event JSON.
- `payload/src/profiler/ui.rs` — the Performance-tab collapsible.
- `payload/src/hooks/profiler.rs` — the profiler-only detours (`DrawGBuffer`,
  `RenderEngine::Draw`, `CGame::UpdateGame`, `ChangeRenderBlockType`); seams the mod already hooks
  for other reasons are instrumented inline in those hooks under `#[cfg(feature = "profiler")]`.

## Deliberate omissions

- The engine's `Graphics::Begin/EndScopeMarker` nullsubs are not detoured, although their ~72
  named call sites survive: they are three-byte `ret` stubs, and a failed micro-detour would abort
  the whole hook library. The highest-value names they carried (the per-render-block-type runs)
  are recovered from `ChangeRenderBlockType` instead; the remaining post-effect-internal names are
  a possible future extension if the coarse seams prove too blunt.
- `g_CpuProfiler` is not read: it holds only zeros in retail.

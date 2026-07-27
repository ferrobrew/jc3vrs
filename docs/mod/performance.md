# Where the frame goes

Measured on 2026-07-26 against a release payload, an RTX 5090, and a headset the compositor paces at 72 Hz (a 13.9 ms budget). Numbers come from the GPU profiler (`docs/mod/profiler.md`); read its caveats before quoting any of them.

## The short version

At the runtime's recommended per-eye resolution the mod is **limited by draw submission and per-draw overhead, not by shading**. The GPU sits at about 50% utilisation while the frame is full. Raising the render resolution is therefore much cheaper than it looks, because the geometry passes barely notice it — the cost of extra pixels lands almost entirely in the fullscreen post passes.

## The measurement that establishes it

Three captures at `vr.resolution_scale` 0.5, 1.0, and 2.0, single-pass stereo on. Halving the scale quarters the pixels; doubling it quadruples them.

| | 0.5x | 1.0x | 2.0x |
|---|---|---|---|
| frame (`CGame::Update`) | 13.57 ms | 13.55 ms | 14.23 ms |
| GPU span | 13.05 ms | 13.25 ms | 14.35 ms |
| busy (upper bound) | 12.26 ms | 12.31 ms | 12.84 ms |
| starved (lower bound) | 0.79 ms (6%) | 0.94 ms (7%) | 1.51 ms (10%) |
| `DrawGBuffer` | 3.64 ms | 3.56 ms | 1.92 ms |
| `PreDraw` | 2.18 ms | 2.16 ms | — |
| `POSTEFFECTS` | — | 1.30 ms | 5.13 ms |
| `DrawPosteffects` | — | 1.26 ms | 4.89 ms |

Two things fall out.

**`DrawGBuffer` does not track pixel count.** It is 3.64 ms at a quarter of the pixels and 3.56 ms at four times that. A pass whose cost is invariant to the number of pixels it fills is bound by something else, and for ~20k draws per frame that something is per-draw work: state changes, descriptor updates, and command recording. Independently, the GPU reports 50% utilisation at 1.0x, which agrees.

**The fullscreen passes do track it, almost exactly.** `POSTEFFECTS` goes 1.30 ms to 5.13 ms for four times the pixels — 3.95x. That is the control: the instrument can see pixel-proportional cost where it exists, so its absence in `DrawGBuffer` is a real finding rather than a blind spot.

The 2.0x column's `DrawGBuffer` (1.92 ms) is lower than at 1.0x, which no resolution effect explains; that capture has a different frame count and almost certainly a different viewpoint. Treat 2.0x as evidence about the post chain only. 2.0x is also where the GPU first reaches 99% utilisation, so it is the crossover into genuine fill limits.

## What single-pass stereo buys

Comparing captures with the feature off and on, at 1.0x:

| | off | on |
|---|---|---|
| dispatches per frame | 2.00 | 1.00 |
| GPU span | 13.86 ms | 13.28 ms |
| starved | 1.05 ms (8%) | 0.79 ms (6%) |
| `DrawGBuffer` | 3.91 ms | 3.47 ms |
| `PreDraw` | 2.29 ms | 2.25 ms |

The collapse works as designed — one dispatch instead of two. The GPU saving is about 0.6 ms, concentrated in the one pass it touches (`DrawGBuffer`, down 11%), which is the expected shape: the same pixels are shaded either way, so what it removes is submission overhead inside that pass. Starvation falls too, which is the right direction — fewer, larger submissions feed the GPU better.

**This comparison predates the collapse's per-eye correctness work.** Since the capture, the deferred
resolve, atmospheric scattering, and the clustered froxel light-grid build have all become per-eye by
default (`docs/mod/single-pass-stereo.md`), each of which runs its pass a second time. The collapse's
draw-submission win is unchanged — it is a property of the one geometry walk — but the "on" column's
GPU numbers now understate the pass cost, and the pair wants re-measuring before either is quoted
again.

Its larger win is on the CPU, and the compositor's 72 Hz cap hides it: frame time is pinned, so headroom shows up as fewer dips rather than a smaller number. `PreDraw` (shadows and reflections) is untouched by design, since those passes fall outside the G-buffer range.

## What this implies

Ordered by expected payoff.

**Submission.** This is where the frame is going, so it is where the wins are. Single-pass stereo is the right lever, but not on the two items that look most inviting. `PreDraw` at 2.16 ms is the second-largest GPU item and sits outside the collapse — but the collapse runs one dispatch, and `PreDraw` runs once per dispatch, so it is already issued once per frame and there is nothing there to collapse. The same goes for the ~450 instanced draws issued outside the G-buffer range (the shadow-pass families). Both are the game's own baseline cost, and the 2.29 ms / 2.25 ms pair above is the measurement that says so: halving the dispatch count barely moves `PreDraw`. See "Why `PreDraw` is outside the collapse" in [`single-pass-stereo.md`](single-pass-stereo.md). What is left on the submission side is the geometry that the collapse still re-issues per eye rather than instance-doubling.

**Fill rate, for the configurations this hardware does not represent.** Higher-resolution headsets and weaker GPUs will bind on fill where this machine does not, and the render is noticeably soft at 1.0x, so resolution wants to go *up*. The asymmetry above makes that affordable: extra pixels cost post-chain time, not geometry time.

- **Foveation** (`payload/src/vr/foveation.rs`) is the structurally correct answer, because it saves more as resolution rises and it discards pixels the optics discard anyway.
- **FSR2** (`payload/src/fsr/`) is already integrated and trades fill for sharpness directly.
- The obvious VR post savings are available but not taken by default: `post_fx.skip_motion_blur`, `post_fx.skip_dof`, and `stereo.skip_ssr` all exist and all default **off**. What remains in the post chain is wanted output — tone mapping, bloom, AA, SSAO — so cutting it further is a quality decision, not free.

**Worth checking before either.** The engine renders at the runtime's recommended size while the OpenXR swapchain keeps the size it was created at, and the eye-to-swapchain step is a scaling shader blit. If those disagree, every frame pays a resample that looks exactly like softness. Both sizes are in the log (`created the stereo swapchain …` against the resize target); confirm they match at 1.0x before attributing blur to resolution.

## Reproducing this

Capture in **release** (`scripts/proton_run.sh --release`) — a debug payload reaches 48 fps where release reaches 90-100, so debug numbers describe the build, not the game. Leave the per-draw render-block scopes off (the default); they add ~300 puffin scopes per frame to the draw path, which is the path under investigation.

Read four numbers per frame, from the periodic log line or the analyzer: busy, starved, idle between dispatches, and CPU submit. Busy is an upper bound and starved a lower bound — idle finer than a marker interval is unobservable and lands in busy. That limit is what made an earlier reading of this same workload look shading-bound when it was not; the resolution A/B above is what settles it, and it is worth repeating rather than inferring.

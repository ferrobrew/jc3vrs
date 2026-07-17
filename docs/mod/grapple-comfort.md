# Grapple comfort

The grapple is JC3's most-used traversal mechanic, and in VR it used to be the most uncomfortable one: firing at anything off-axis swung the view violently, with rotation the inner ear never felt. This doc describes the filter that fixes it (issue #36). The implementation lives in `payload/src/grapple/`; the engine-side grapple machinery it reads is documented in `docs/engine/grapple-pipeline.md`.

## The problem: rotation applied twice

You aim the grapple by looking at the target, so the head pose already carries the rotation toward it. The engine then rotates the character's root toward the same target — first the fire act's directional alignment (up to 130° at ~1000°/s, before the wire even spawns), then `NReeledInController::RotateToGrappleTarget` during the reel. The headpose composes the view as `body × head`, so that rotation lands twice and the view sweeps *past* the thing you're looking at.

The fix filters the body-driven inputs to that composition while the grapple owns the character. The HMD's own tracking passes through untouched in both rotation and position, per the "no smoothing on the HMD→camera path" rule in `head-and-body.md`. Three pieces cooperate: the body-frame hold, the yaw handoff, and the landing-snap absorber.

## The body-frame hold

While the grapple is active, `filter_body_rotation` blends the body rotation the head composes onto toward a filtered frame, per `headpose.grapple.mode`:

- **Hold view** (default): the frame the previous render composed with, held from fire to landing. The view stays world-stable — what you're looking at stays looked-at — and only the HMD moves it. This cancels exactly the rotation the grapple *adds*, in both pitch and yaw, while preserving whatever frame you entered with (a banked wingsuit stays banked).
- **Level pitch**: the live frame flattened to its yaw. The view stays level but still turns with the body's heading. Kept for players who prefer the view to follow the reel's direction; it leaves the yaw half of the double-count in place.
- **Off**: no filtering.

### When the hold is active

The grapple's own state field is not a reliable activity signal, so the filter reads three hook fields together (`GrapplingHook` in the pyxis defs):

| Phase | Signal |
|---|---|
| Fire committed (act aligning the body) | `m_WaitingForGrappleFire` / `m_WaitingForTetherFire` |
| Hook in flight | `GHS_INACTIVE` with a live `m_ActiveWire` |
| Zipping | `GHS_REELING_IN` |
| Attached (wall perch, hang) | `GHS_REELED_*` with a live `m_ActiveWire` |

Two of these gates exist because the obvious signal lies. The fire flags matter because the body starts whipping toward the target before any wire exists — engaging on `GHS_REELING_IN` alone leaves that whole alignment in the view. And the `GHS_REELED_*` states need the wire check because the engine treats `m_State` as "last reel outcome" as much as "current state": telemetry caught it parked at `GHS_REELED_ATTACHED` for six minutes of ordinary play, which without the gate kept the filter engaged through normal locomotion.

### Engage and release

Engagement is instant and seamless by construction: on the engage edge, the filter holds the frame the view *last composed* — the previous advance's body under the current blend. The previous advance's body, because the fire snap lands in the same step the state flips (capturing the current body bakes 5–27° of it into the hold). Under the current blend, because a re-grapple during a release tail must hold the partially blended frame you're already looking through.

Release blends the accumulated rotation back over `release_s` (0.4 s), and the release tail snaps to fully disarmed below 0.1% blend — the decay is exponential and would otherwise linger near-zero for many seconds, leaving stale state for the next engage.

## The yaw handoff

An on-foot landing leaves the body heading 10–25° off the held view, and returning that through the release blend is 34–61° of rotation path at sustained vection. With `yaw_handoff` (default on, VR on-foot hold-view only), the view never sweeps through it: at reel end the held heading is posted for the VR body-turn accumulator (`grapple::take_body_yaw_retarget`, consumed on its next on-foot tick), the *character* turns to face where you're looking via the game's own rate-limited turn machinery, and the hold stays engaged until the heading converges within 3° — leaving only a small pitch/roll residual for the blend. This is the design doc's body-follows-head scheme applied at the landing.

The handoff falls back to the plain blend release on a timeout (`handoff_timeout_s`, the body can be blocked from turning) and skips entirely for airborne releases: the body-yaw accumulator only steers on foot, so holding the view while flying away would pin it against a moving world.

Because the held frame's yaw stops tracking the real body, everything that steers the actual character reads the raw rotation (`headpose::xr::body_rotation_raw`); only the view composition reads the filtered one.

## The landing-snap absorber

The attach/landing animation teleports the head anchor up to a metre in a single sim tick — a 50–100+ m/s velocity spike, against ~43 m/s for the fastest real zips. `filter_anchor` absorbs that spike into an offset that decays out of the view over `anchor_snap_ease_s`, leaving sustained motion of any speed untouched.

The detection compares each anchor step against a smoothed velocity estimate and absorbs only single-step changes beyond `anchor_snap_threshold_mps`. Three details carry the correctness, each the fix for a failure telemetry caught:

- **Steps, not advances.** The anchor is tick-sampled, constant across the frames between sim ticks, so per-advance deltas form a `0, 0, full-step` staircase that a per-advance model misreads as perpetual excess at ordinary movement speeds. Detection runs only when the anchor actually changes, with the step interval measured between changes.
- **The estimate always tracks.** Excluding absorbed steps from the velocity estimate turns the absorber into a speed governor — every zip step reads as excess against a frozen estimate, and the view settles metres behind the body.
- **Armed only from the landing onward.** The zip itself, including its launch acceleration, passes through bit-exact; `GHS_REELING_IN` disarms the absorber, and the only snap the grapple produces is at the attach.

The filtered anchor is always `raw + offset`, so pose-pair deltas built from it keep exact tick spacing and the engine's sub-frame interpolation is undisturbed. The offset is hard-capped at 1 m (a mis-estimate degrades into a bounded, decaying lag), and steps beyond 5 m are genuine teleports — passed through with the absorber reset.

## Cadence and locking

The filter advances on the engine's input tick *and* on every rendered VR frame. The frame-cadence advance is load-bearing: the body starts rotating the instant the grapple acts, and a tick-only (~33 Hz) read leaves up to a tick of that rotation in the view before the filter can engage.

Both advances share one mutex, and it is the innermost lock in the headpose's SIM → BODY_YAW → FILTER order: the input-tick caller holds the sim lock when it calls `advance`, so nothing under the filter's lock may call back into a locking `headpose` accessor — `on_foot` arrives as a parameter for exactly this reason. Violating this deadlocked the game on injection once; see the lock-order note in `payload/src/grapple/mod.rs`.

## Telemetry

`payload/src/grapple/telemetry.rs` captures the filter's inputs and outputs — hook phase, blend, raw/filtered/held body frames, HMD cockpit pose, composed head pose, anchor — to a timestamped `jc3vrs-grapple-<stamp>.csv` beside the payload DLL, one row per input tick and per rendered VR frame. Toggle it from the Camera tab ("Log reel telemetry"; off by default); each enable starts a fresh file. Every behaviour above was diagnosed and verified against these captures, and they remain the tool of choice when a comfort report is hard to reproduce by feel.

## Configuration

`headpose.grapple`, live-editable in the debug UI's Camera tab (with a live blend readout):

| Field | Default | Meaning |
|---|---|---|
| `mode` | `HoldView` | Body-frame filter mode (see above). |
| `engage_s` | `0` | Blend-in time constant. Instant is seamless; a ramp only leaks the opening rotation. |
| `release_s` | `0.4` | Blend-out time constant for the accumulated rotation. |
| `yaw_handoff` | `true` | Turn the character to the view at an on-foot landing instead of sweeping the view. |
| `handoff_timeout_s` | `1.5` | Fallback to the blend release if the body cannot converge. |
| `anchor_snap_threshold_mps` | `25` | Single-step anchor velocity change treated as a landing snap. `0` disables. |
| `anchor_snap_ease_s` | `0.15` | How long an absorbed snap takes to ease back out. |

# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Analyze a jc3vrs profiler capture (`jc3vrs-profile-*.json`, issue #34).

Reads the Chrome trace-event JSON the in-game profiler dumps (F9 / the Performance tab) and
reports where the frame budget goes:

  * frame-time statistics and an ASCII histogram against the HMD budgets,
  * the main-thread phase budget (sim, render update, per-dispatch submit vs. draw-thread wait),
  * the GPU lane per dispatch kind (eyes, far field) and per render seam,
  * the draw-thread CPU cost per seam, per render pass, and per render-block type,
  * the worst frames with their per-phase and per-GPU-dispatch breakdown.

Reading notes baked into the numbers:
  * The `Dispatch` scope covers its whole loop iteration, so the CPU submit cost is reported as
    `Dispatch` minus the contained `WaitForCPUDraw + drain` scope.
  * The far dispatch runs at ordinal 0, so its PreDraw seam carries the frame's *shared*
    prepasses (sun-shadow atlas, reflections, water sim) — that cost is once per frame, not a
    far-field overhead.
  * GPU results report a few frames late; events are attributed to frames by wall-clock windows,
    so per-frame GPU numbers describe "the GPU work that resolved during that frame".

Usage: uv run tools/analyze_profile.py <jc3vrs-profile-*.json> [--worst N] [--top N] [--csv out.csv]
"""

import argparse
import bisect
import json
import statistics
import sys
from collections import defaultdict

# The HMD frame budgets the histogram marks, in milliseconds.
BUDGETS_MS = [1000 / 120, 1000 / 90, 1000 / 72, 1000 / 45]

# The GPU lane's outer (per-dispatch) scope names.
GPU_LANES = ["GPU eye 0", "GPU eye 1", "GPU far field"]

# The render seams, in draw order (shared by the CPU draw lane and the GPU lane).
SEAMS = ["PreDraw", "DrawGBuffer", "Draw (scene)", "DrawPosteffects", "PostDraw"]
CPU_SEAM_NAMES = {
    "RenderEngine::PreDraw": "PreDraw",
    "DrawGBuffer": "DrawGBuffer",
    "Draw (scene)": "Draw (scene)",
    "RenderEngine::DrawPosteffects": "DrawPosteffects",
    "RenderEngine::PostDraw": "PostDraw",
}


def load(path: str) -> tuple[dict[int, str], list[dict]]:
    """Returns (tid -> lane name, X events sorted by ts)."""
    with open(path) as f:
        events = json.load(f)
    lanes = {e["tid"]: e["args"]["name"] for e in events if e.get("ph") == "M"}
    xs = [e for e in events if e.get("ph") == "X"]
    xs.sort(key=lambda e: (e["ts"], -e["dur"]))
    return lanes, xs


def build_hierarchy(xs: list[dict]) -> None:
    """Annotates each event with `parent` (an event or None) via per-tid containment stacks.

    The exporter writes each thread's scopes in depth-first order with correct containment, so a
    stack sweep in (ts, -dur) order reconstructs the tree exactly.
    """
    stacks: dict[int, list[dict]] = defaultdict(list)
    for e in xs:
        stack = stacks[e["tid"]]
        while stack and e["ts"] >= stack[-1]["ts"] + stack[-1]["dur"] - 1e-3:
            stack.pop()
        e["parent"] = stack[-1] if stack else None
        stack.append(e)


def pctl(sorted_ms: list[float], p: float) -> float:
    if not sorted_ms:
        return 0.0
    return sorted_ms[min(len(sorted_ms) - 1, int(len(sorted_ms) * p))]


def stats_line(ms: list[float]) -> str:
    s = sorted(ms)
    return (
        f"n={len(s):5}  mean {statistics.mean(s):7.2f}  p50 {pctl(s, 0.5):7.2f}  "
        f"p95 {pctl(s, 0.95):7.2f}  p99 {pctl(s, 0.99):7.2f}  max {max(s):7.2f} ms"
    )


def histogram(ms: list[float], width: int = 50, bins: int = 18) -> list[str]:
    lo, hi = min(ms), max(ms)
    span = max(hi - lo, 1e-6)
    counts = [0] * bins
    for v in ms:
        counts[min(bins - 1, int((v - lo) / span * bins))] += 1
    peak = max(counts)
    lines = []
    for i, c in enumerate(counts):
        b0 = lo + span * i / bins
        b1 = lo + span * (i + 1) / bins
        marks = "".join(" ←%.0fHz" % (1000 / b) for b in BUDGETS_MS if b0 <= b < b1)
        lines.append(f"  {b0:6.1f}–{b1:6.1f} ms |{'#' * int(c / peak * width):<{width}}| {c}{marks}")
    return lines


class Frames:
    """Frame windows from the game lane's `CGame::Update` scopes, for wall-clock attribution."""

    def __init__(self, frame_events: list[dict]):
        self.events = frame_events
        self.starts = [e["ts"] for e in frame_events]

    def index_of(self, ts: float) -> int | None:
        i = bisect.bisect_right(self.starts, ts) - 1
        return i if i >= 0 else None


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("trace", help="a jc3vrs-profile-*.json capture")
    ap.add_argument("--worst", type=int, default=5, help="worst frames to detail (default 5)")
    ap.add_argument("--top", type=int, default=15, help="rows in the pass/type rankings (default 15)")
    ap.add_argument("--csv", help="also write a per-frame CSV to this path")
    args = ap.parse_args()

    lanes, xs = load(args.trace)
    build_hierarchy(xs)
    by_lane: dict[str, list[dict]] = defaultdict(list)
    for e in xs:
        by_lane[lanes.get(e["tid"], str(e["tid"]))].append(e)
    game = by_lane.get("game", [])
    draw = [e for lane, evs in by_lane.items() if lane == "draw" for e in evs]
    gpu = by_lane.get("GPU", [])
    if not game:
        sys.exit("analyze_profile: no 'game' lane in the trace — is this a jc3vrs profiler capture?")

    frame_events = [e for e in game if e["name"] == "CGame::Update"]
    frames = Frames(frame_events)
    frame_ms = [e["dur"] / 1000 for e in frame_events]
    span_s = (xs[-1]["ts"] + xs[-1]["dur"] - xs[0]["ts"]) / 1e6
    # The engine's draw task hops between CPU-fragment worker threads; each OS thread is its own
    # trace lane, all labelled "draw".
    n_draw_lanes = sum(1 for name in lanes.values() if name == "draw")

    dispatches = [e for e in game if e["name"] == "Dispatch"]
    per_frame_dispatches = len(dispatches) / max(len(frame_events), 1)

    print(f"== {args.trace}")
    print(
        f"span {span_s:.2f} s · {len(frame_events)} frames ({len(frame_events) / span_s:.1f} fps) · "
        f"{len(dispatches)} dispatches ({per_frame_dispatches:.1f}/frame"
        f"{', far-field share frames' if per_frame_dispatches > 2.5 else ''}) · "
        f"{len(xs)} scopes · draw lanes: {n_draw_lanes}"
    )

    print("\n== frame time")
    print(f"  {stats_line(frame_ms)}")
    for line in histogram(frame_ms):
        print(line)

    # -- main-thread phase budget ---------------------------------------------------------------
    print("\n== main thread, per frame (ms)")
    n = max(len(frame_events), 1)

    def game_total(name: str) -> float:
        return sum(e["dur"] for e in game if e["name"] == name) / 1000

    update_game = game_total("CGame::UpdateGame")
    update_render = game_total("CGame::UpdateRender")
    wait = game_total("WaitForCPUDraw + drain")
    dispatch_total = game_total("Dispatch")
    submit = dispatch_total - wait  # Dispatch contains the wait scope.
    frame_total = sum(frame_ms)
    print(f"  sim (UpdateGame)              {update_game / n:7.2f}")
    print(f"  render update (UpdateRender)  {update_render / n:7.2f}")
    print(f"    dispatch submit             {submit / n:7.2f}")
    print(f"    draw-thread wait            {wait / n:7.2f}   <- main thread blocked on the draw thread")
    print(f"    other render update         {(update_render - dispatch_total) / n:7.2f}")
    print(f"  other frame                   {(frame_total - update_game - update_render) / n:7.2f}")

    known = {"CGame::Update", "CGame::UpdateGame", "CGame::UpdateRender", "Dispatch", "WaitForCPUDraw + drain"}
    extra: dict[str, tuple[float, int]] = defaultdict(lambda: (0.0, 0))
    for e in game:
        if e["name"] not in known:
            t, c = extra[e["name"]]
            extra[e["name"]] = (t + e["dur"] / 1000, c + 1)
    if extra:
        print("\n== other main-thread scopes, per frame (ms; may nest inside the phases above)")
        for name, (total, count) in sorted(extra.items(), key=lambda kv: -kv[1][0]):
            print(f"  {total / n:7.2f}  n={count:6}  {name}")

    # -- GPU ------------------------------------------------------------------------------------
    if gpu:
        print("\n== GPU, per dispatch kind (ms)")
        outer_events = [e for e in gpu if e["name"] in GPU_LANES]
        gpu_frame_total = 0.0
        for lane_name in GPU_LANES:
            ds = [e["dur"] / 1000 for e in outer_events if e["name"] == lane_name]
            if not ds:
                continue
            gpu_frame_total += sum(ds) / n
            print(f"  {lane_name:16} {stats_line(ds)}")
        print(f"  total GPU/frame ≈ {gpu_frame_total:.2f} ms (budget {1000 / 90:.1f} @90Hz, {1000 / 45:.1f} @45Hz)")

        # The measured starvation bubbles between dispatches ("GPU idle" scopes; only present in
        # captures taken after the bubble measurement landed).
        idle = [e["dur"] / 1000 for e in gpu if e["name"] == "GPU idle"]
        if idle:
            idle_frame = sum(idle) / n
            util = gpu_frame_total / max(gpu_frame_total + idle_frame, 1e-6)
            print(f"  measured inter-dispatch idle ≈ {idle_frame:.2f} ms/frame -> GPU utilization {util * 100:.0f}% while rendering")
            print(f"  {'GPU idle':16} {stats_line(idle)}")

        print("\n== GPU seams within each dispatch kind (mean ms per dispatch)")
        seam_sum: dict[tuple[str, str], float] = defaultdict(float)
        seam_n: dict[str, int] = defaultdict(int)
        for e in outer_events:
            seam_n[e["name"]] += 1
        for e in gpu:
            if e["parent"] is not None and e["parent"]["name"] in GPU_LANES:
                seam_sum[(e["parent"]["name"], e["name"])] += e["dur"] / 1000
        header = "".join(f"{s:>17}" for s in SEAMS)
        print(f"  {'':16}{header}")
        for lane_name in GPU_LANES:
            if seam_n[lane_name] == 0:
                continue
            row = "".join(
                f"{seam_sum[(lane_name, s)] / seam_n[lane_name]:17.2f}" for s in SEAMS
            )
            print(f"  {lane_name:16}{row}")
        print("  (far-field PreDraw = the frame's shared prepasses: shadow atlas, reflections, water)")

    # -- draw thread ----------------------------------------------------------------------------
    if draw:
        print("\n== draw thread CPU, per seam (mean ms per dispatch)")
        n_disp = max(len(dispatches), 1)
        for cpu_name, seam in CPU_SEAM_NAMES.items():
            total = sum(e["dur"] for e in draw if e["name"] == cpu_name) / 1000
            if total:
                print(f"  {seam:16} {total / n_disp:7.2f}")

        def ranking(pred, label: str) -> None:
            agg: dict[str, tuple[float, int]] = defaultdict(lambda: (0.0, 0))
            for e in draw:
                if pred(e):
                    t, c = agg[e["name"]]
                    agg[e["name"]] = (t + e["dur"] / 1000, c + 1)
            rows = sorted(agg.items(), key=lambda kv: -kv[1][0])[: args.top]
            print(f"\n== top {label} by draw-thread CPU (total ms over capture / per frame / count)")
            for name, (total, count) in rows:
                print(f"  {total:9.1f}  {total / n:7.3f}  {count:7}  {name}")

        ranking(
            lambda e: e["parent"] is not None and e["parent"]["name"] == "DrawRenderPassRange",
            "render passes",
        )
        ranking(
            lambda e: e["parent"] is not None
            and e["parent"]["parent"] is not None
            and e["parent"]["parent"]["name"] == "DrawRenderPassRange",
            "render-block types",
        )

    # -- worst frames ---------------------------------------------------------------------------
    print(f"\n== worst {args.worst} frames")
    order = sorted(range(len(frame_events)), key=lambda i: -frame_events[i]["dur"])[: args.worst]
    gpu_outer = [e for e in gpu if e["name"] in GPU_LANES]
    for i in sorted(order):
        fe = frame_events[i]
        t0, t1 = fe["ts"], fe["ts"] + fe["dur"]
        in_frame = lambda e: t0 <= e["ts"] < t1  # noqa: E731
        parts = {
            name: sum(e["dur"] for e in game if e["name"] == name and in_frame(e)) / 1000
            for name in ["CGame::UpdateGame", "WaitForCPUDraw + drain"]
        }
        gpu_parts = {
            lane_name: sum(e["dur"] for e in gpu_outer if e["name"] == lane_name and in_frame(e)) / 1000
            for lane_name in GPU_LANES
        }
        gpu_str = "  ".join(f"{k.removeprefix('GPU ')}={v:.1f}" for k, v in gpu_parts.items() if v)
        print(
            f"  frame {i:4} @ {(t0 - frame_events[0]['ts']) / 1e6:6.2f}s  {fe['dur'] / 1000:6.2f} ms"
            f"  sim={parts['CGame::UpdateGame']:.1f}  wait={parts['WaitForCPUDraw + drain']:.1f}"
            f"  gpu[{gpu_str}]"
        )

    # -- CSV ------------------------------------------------------------------------------------
    if args.csv:
        import csv

        with open(args.csv, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["frame", "start_s", "frame_ms", "sim_ms", "wait_ms"] + GPU_LANES)
            for i, fe in enumerate(frame_events):
                t0, t1 = fe["ts"], fe["ts"] + fe["dur"]
                sim = sum(e["dur"] for e in game if e["name"] == "CGame::UpdateGame" and t0 <= e["ts"] < t1) / 1000
                wt = sum(e["dur"] for e in game if e["name"] == "WaitForCPUDraw + drain" and t0 <= e["ts"] < t1) / 1000
                gp = [
                    sum(e["dur"] for e in gpu_outer if e["name"] == ln and t0 <= e["ts"] < t1) / 1000
                    for ln in GPU_LANES
                ]
                w.writerow([i, f"{(t0 - frame_events[0]['ts']) / 1e6:.3f}", f"{fe['dur'] / 1000:.3f}", f"{sim:.3f}", f"{wt:.3f}"] + [f"{v:.3f}" for v in gp])
        print(f"\nper-frame CSV -> {args.csv}")


if __name__ == "__main__":
    main()

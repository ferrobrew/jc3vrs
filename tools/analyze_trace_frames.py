# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "pillow"]
# ///
"""Analyze a render-trace screenshot capture to localize a per-frame flicker.

Reads `traces/<stamp>/frameNNN_eye{0,1}.png` plus `trace.ndjson`, and reports:
  * per-frame luminance distribution and the frame-to-frame brightness pulse,
  * a per-pixel temporal-standard-deviation heatmap (where the flicker lives),
  * a coarse tile ranking of which regions vary most across frames,
  * correlation of the brightness pulse with the exposure / shadow-cascade series.

Usage: uv run tools/analyze_trace_frames.py <traces/stamp dir> [--eye 0]
"""

import argparse
import json
import pathlib
import sys

import numpy as np
from PIL import Image

REC709 = np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)


def luma(img: np.ndarray) -> np.ndarray:
    return img[..., :3].astype(np.float32) @ REC709


def load_frames(dir_: pathlib.Path, eye: int):
    paths = sorted(dir_.glob(f"frame*_eye{eye}.png"))
    if not paths:
        sys.exit(f"no frame*_eye{eye}.png in {dir_}")
    return paths


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("dir", type=pathlib.Path)
    ap.add_argument("--eye", type=int, default=0)
    ap.add_argument("--down", type=int, default=4, help="downscale factor for the temporal maps")
    args = ap.parse_args()

    paths = load_frames(args.dir, args.eye)
    n = len(paths)
    full = np.asarray(Image.open(paths[0]))
    h, w = full.shape[:2]
    print(f"{n} frames, {w}x{h}, eye {args.eye}")

    # Per-eye divergence: the mean-luma gap between eye 0 and eye 1 versus each eye's own temporal wobble.
    # A gap that dwarfs the wobble (and non-overlapping ranges) is a fixed per-eye render difference, not a
    # global fluctuation aliased by capture timing.
    e1_paths = sorted(args.dir.glob("frame*_eye1.png"))
    e0_paths = sorted(args.dir.glob("frame*_eye0.png"))
    if e0_paths and e1_paths:
        s0 = np.array([float(luma(np.asarray(Image.open(p))).mean()) for p in e0_paths])
        s1 = np.array([float(luma(np.asarray(Image.open(p))).mean()) for p in e1_paths])
        gap = s0.mean() - s1.mean()
        wob = max(s0.std(), s1.std())
        sep = "SEPARATED" if s0.min() > s1.max() or s1.min() > s0.max() else "overlapping"
        print("\n=== per-eye divergence ===")
        print(f"eye0 mean={s0.mean():.2f} std={s0.std():.2f} [{s0.min():.1f},{s0.max():.1f}]")
        print(f"eye1 mean={s1.mean():.2f} std={s1.std():.2f} [{s1.min():.1f},{s1.max():.1f}]")
        print(
            f"gap={gap:+.2f} ({abs(gap) / s0.mean() * 100:.1f}%)  = {abs(gap) / max(wob, 1e-6):.1f}x the "
            f"temporal wobble  ranges {sep}"
        )

    # Per-frame engine parity (m_FrameIndex & 1) from the trace, so the even/odd split matches the actual
    # shadow-atlas ping-pong, not just the capture index. Falls back to capture-index parity.
    parity = {}
    nd = args.dir / "trace.ndjson"
    if nd.exists():
        for line in nd.open():
            try:
                o = json.loads(line)
            except Exception:
                continue
            e = o.get("event", {})
            if e.get("ev") == "ShadowState" and o.get("eye") == args.eye:
                parity[o["frame"]] = e.get("frame_index", o["frame"]) & 1
    par = [parity.get(i, i & 1) for i in range(n)]

    # Streaming Welford accumulators for a per-pixel temporal mean/variance at reduced resolution,
    # plus full-frame and tile luminance series.
    dw, dh = w // args.down, h // args.down
    count = 0
    mean = np.zeros((dh, dw), np.float64)
    m2 = np.zeros((dh, dw), np.float64)
    frame_means = []
    TILES = 12
    tile_series = np.zeros((n, TILES, TILES), np.float64)
    # Parity accumulators (downscaled) to isolate the ping-pong from head-movement noise.
    psum = [np.zeros((dh, dw), np.float64), np.zeros((dh, dw), np.float64)]
    pcnt = [0, 0]

    for i, p in enumerate(paths):
        img = np.asarray(Image.open(p))
        lum = luma(img)
        frame_means.append(float(lum.mean()))
        # tile means
        ys = np.linspace(0, h, TILES + 1).astype(int)
        xs = np.linspace(0, w, TILES + 1).astype(int)
        for ty in range(TILES):
            for tx in range(TILES):
                tile_series[i, ty, tx] = lum[ys[ty]:ys[ty + 1], xs[tx]:xs[tx + 1]].mean()
        # downscaled luminance for the heatmap (block-average)
        small = lum[: dh * args.down, : dw * args.down].reshape(dh, args.down, dw, args.down).mean((1, 3))
        count += 1
        delta = small - mean
        mean += delta / count
        m2 += delta * (small - mean)
        b = par[i]
        psum[b] += small
        pcnt[b] += 1

    var = m2 / max(count - 1, 1)
    std = np.sqrt(var)
    frame_means = np.array(frame_means)

    print("\n=== full-frame luminance (0..255) ===")
    print(f"mean={frame_means.mean():.3f}  min={frame_means.min():.3f}  max={frame_means.max():.3f}"
          f"  peak-to-peak={frame_means.max() - frame_means.min():.3f}"
          f"  ({(frame_means.max() - frame_means.min()) / frame_means.mean() * 100:.2f}% of mean)")
    d = np.abs(np.diff(frame_means))
    print(f"frame-to-frame |delta| mean={d.mean():.4f} max={d.max():.4f}")
    print("\nper-frame mean and delta (first 30):")
    for i in range(min(30, n)):
        dd = f"{frame_means[i] - frame_means[i-1]:+.3f}" if i else "   -"
        print(f"  f{i:03d} {frame_means[i]:8.3f}  d={dd}")

    print("\n=== where it varies: temporal std, downscaled ===")
    print(f"pixel std: mean={std.mean():.3f}  median={np.median(std):.3f}  p95={np.percentile(std,95):.3f}  max={std.max():.3f}")
    # is the variation global (uniform std) or local (peaky std)?
    print(f"std uniformity: p95/median = {np.percentile(std,95)/max(np.median(std),1e-6):.2f}"
          f"  (near 1 => global/uniform pulse; large => localized)")

    print("\n=== tile temporal std (12x12 grid, higher = flickers more) ===")
    tile_std = tile_series.std(0)
    # print as a grid
    for ty in range(TILES):
        print("  " + " ".join(f"{tile_std[ty, tx]:4.1f}" for tx in range(TILES)))
    flat = [(tile_std[ty, tx], ty, tx) for ty in range(TILES) for tx in range(TILES)]
    flat.sort(reverse=True)
    print("  top tiles (std, row, col):", [(round(s, 2), r, c) for s, r, c in flat[:6]])
    print(f"  tile std: max={tile_std.max():.3f} median={np.median(tile_std):.3f}"
          f" ratio={tile_std.max()/max(np.median(tile_std),1e-6):.2f}")

    # Save the std heatmap (normalized) for visual inspection.
    hm = (std / max(std.max(), 1e-6) * 255).astype(np.uint8)
    out = args.dir / f"_stdmap_eye{args.eye}.png"
    Image.fromarray(hm).save(out)
    print(f"\nstd heatmap -> {out}")

    # === parity (ping-pong) analysis ===
    print("\n=== parity (engine m_FrameIndex & 1) analysis ===")
    fm = frame_means
    even = np.array([fm[i] for i in range(n) if par[i] == 0])
    odd = np.array([fm[i] for i in range(n) if par[i] == 1])
    print(f"parity 0 mean={even.mean():.3f} (n={len(even)})  parity 1 mean={odd.mean():.3f} (n={len(odd)})"
          f"  gap={abs(even.mean()-odd.mean()):.3f} ({abs(even.mean()-odd.mean())/fm.mean()*100:.2f}%)")
    # lag-1 autocorrelation: strong negative => clean every-other-frame alternation.
    x = fm - fm.mean()
    ac1 = float((x[:-1] * x[1:]).sum() / (x * x).sum())
    ac2 = float((x[:-2] * x[2:]).sum() / (x * x).sum())
    print(f"autocorr lag1={ac1:+.3f} (negative => parity alternation)  lag2={ac2:+.3f}")

    if pcnt[0] and pcnt[1]:
        pdiff = psum[0] / pcnt[0] - psum[1] / pcnt[1]  # even minus odd, per pixel
        print(f"per-pixel parity diff: mean={pdiff.mean():+.3f} absmean={np.abs(pdiff).mean():.3f}"
              f" p99={np.percentile(np.abs(pdiff),99):.3f} max={np.abs(pdiff).max():.3f}")
        # Signed visualization: gray=0, bright=parity0 brighter, dark=parity1 brighter.
        span = max(np.percentile(np.abs(pdiff), 99), 1e-6)
        vis = np.clip(pdiff / span * 127 + 128, 0, 255).astype(np.uint8)
        pout = args.dir / f"_paritydiff_eye{args.eye}.png"
        Image.fromarray(vis).save(pout)
        print(f"parity-diff map -> {pout}  (mid-gray = no parity flicker; light/dark = flickers)")

    # Correlate with trace exposure / shadow series if present.
    nd = args.dir / "trace.ndjson"
    if nd.exists():
        expo = {}
        for line in nd.open():
            try:
                o = json.loads(line)
            except Exception:
                continue
            e = o.get("event", {})
            if e.get("ev") == "ExposureInternals" and o.get("eye") == args.eye:
                expo[o["frame"]] = e.get("exposure")
        if expo:
            ex = np.array([expo.get(i, np.nan) for i in range(n)])
            ok = ~np.isnan(ex)
            if ok.sum() > 2:
                c = np.corrcoef(frame_means[ok], ex[ok])[0, 1]
                print(f"\n=== exposure ===")
                print(f"exposure range=[{np.nanmin(ex):.5f},{np.nanmax(ex):.5f}]"
                      f" pinned={np.nanmax(ex)-np.nanmin(ex) < 1e-6}")
                print(f"corr(frame brightness, exposure) = {c:+.3f}")


if __name__ == "__main__":
    main()

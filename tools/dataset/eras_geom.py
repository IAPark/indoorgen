"""Geometry for make_eras.py: depth unprojection / reprojection between cameras (OpenCV convention: x right, y down, z forward;
c2w 4x4 world z up, metres; depth = z-depth, 0 = no surface), video path candidates and their scores, reference-photo overlap,
per-frame "what the references show" masks, and the checks of the per-home quality gate."""
import math
import numpy as np

# mask classes of a target-video pixel against the reference photos (other eras + the target era's own photos)
NEVER, SEEN, CHANGED, HIDDEN = 0, 1, 2, 3
MASK_LEGEND = {NEVER: "no reference looks at this surface", SEEN: "a reference shows this same surface (unchanged)",
               CHANGED: "a reference looked along this ray and saw something else (moved / new / removed object, person)",
               HIDDEN: "inside a reference's view but behind something there (an occluder, or an object since removed)"}


def K_of(f_mm, W, H, cx=None, cy=None):
    f = f_mm / 36.0 * W
    return np.array([[f, 0, W / 2 if cx is None else cx], [0, f, H / 2 if cy is None else cy], [0, 0, 1.0]])


def unproject(depth, K, c2w, stride=1):
    """-> (N, 3) world points of the pixels with depth > 0 (pixel centres), and their (row, col)"""
    d = depth[::stride, ::stride]
    v, u = np.nonzero(d > 0)
    z = d[v, u]
    x = ((u * stride + 0.5) - K[0, 2]) / K[0, 0] * z
    y = ((v * stride + 0.5) - K[1, 2]) / K[1, 1] * z
    pc = np.stack([x, y, z, np.ones_like(z)], 1)
    pw = pc @ np.asarray(c2w).T
    return pw[:, :3], v * stride, u * stride


def project(pw, K, w2c):
    """world points -> (u, v, z) in a camera (pixel coordinates, z-depth)"""
    pc = np.c_[pw, np.ones(len(pw))] @ np.asarray(w2c).T
    z = pc[:, 2]
    with np.errstate(divide="ignore", invalid="ignore"):
        u = K[0, 0] * pc[:, 0] / z + K[0, 2]
        v = K[1, 1] * pc[:, 1] / z + K[1, 2]
    return u, v, z


def classify(pw, refs, tol_rel=0.03, tol_abs=0.04):
    """class per world point against the reference views [(depth, K, w2c)]: SEEN beats CHANGED beats HIDDEN beats NEVER"""
    cls = np.full(len(pw), NEVER, np.uint8)
    for depth, K, w2c in refs:
        H, W = depth.shape
        u, v, z = project(pw, K, w2c)
        ok = (z > 0.05) & (u >= 0) & (u < W) & (v >= 0) & (v < H)
        idx = np.nonzero(ok)[0]
        if not len(idx): continue
        dr = depth[v[idx].astype(int), u[idx].astype(int)]
        zz = z[idx]; tol = tol_abs + tol_rel * zz
        seen = (dr > 0) & (np.abs(zz - dr) < tol)
        # (the reference saw past where this surface is now: it was not there then -- or a void, a window, counts as past)
        changed = ((dr <= 0) | (dr > zz + tol)) & ~seen
        hidden = (dr > 0) & (dr < zz - tol)
        c = cls[idx]
        c = np.where(seen, SEEN, np.where((c != SEEN) & changed, CHANGED, np.where((c == NEVER) & hidden, HIDDEN, c)))
        cls[idx] = c
    return cls


OUTSIDE = 255   # mask value of pixels past FAR: the outdoors through a window, not counted in the shares
FAR = 12.0


def frame_mask(depth, K, c2w, refs):
    """per-pixel class image (uint8: 0 where the frame has no surface, NEVER; OUTSIDE past FAR) of one video frame"""
    m = np.zeros(depth.shape, np.uint8)
    d = np.where(depth > FAR, 0.0, depth)
    pw, v, u = unproject(d, K, c2w)
    if len(pw): m[v, u] = classify(pw, refs)
    m[depth > FAR] = OUTSIDE
    return m


def overlap(pw, K, w2c, W, H, depth=None, max_z=10.0):
    """share of world points a camera sees: inside its frustum (and, with its depth, on the surface it shows)"""
    if not len(pw): return 0.0
    u, v, z = project(pw, K, w2c)
    ok = (z > 0.1) & (z < max_z) & (u >= 0) & (u < W) & (v >= 0) & (v < H)
    if depth is not None:
        idx = np.nonzero(ok)[0]
        dr = depth[v[idx].astype(int), u[idx].astype(int)]
        good = (dr > 0) & (np.abs(z[idx] - dr) < 0.04 + 0.03 * z[idx])
        return float(good.sum()) / len(pw)
    return float(ok.sum()) / len(pw)


# ------------------------------------------------------------------------------------------------ video paths
def _wrap(a): return (a + math.pi) % (2 * math.pi) - math.pi


def _fwd(yaw, pitch): return np.array([math.cos(pitch) * math.cos(yaw), math.cos(pitch) * math.sin(yaw), math.sin(pitch)])


def move_paths(view, rng):
    """camera moves from one valid pose: (family, [(pos, yaw, pitch)] key poses). Families as in walk-through video: push in, pull back,
    pan, orbit about what the view looks at, strafe, walk and turn"""
    p = np.array(view["pos"], float); y, pt = view["yaw"], view["pitch"]
    f = _fwd(y, 0.0); r = np.array([math.sin(y), -math.cos(y), 0.0])
    hit = np.array(view.get("hit", p + 2.5 * _fwd(y, pt)), float)
    d = float(np.linalg.norm((hit - p)[:2])) or 2.0
    s = rng.choice([-1, 1])
    out = [("push", [(p, y, pt), (p + f * min(1.6, 0.55 * d), y, pt)]),
           ("pull", [(p + f * min(1.2, 0.45 * d), y, pt), (p, y, pt)]),
           ("pan", [(p, y - s * math.radians(35), pt), (p, y + s * math.radians(35), pt)]),
           ("strafe", [(p - r * 0.7, y, pt), (p + r * 0.7, y, pt)]),
           ("walkturn", [(p, y, pt), (p + f * 0.8, y + s * math.radians(25), pt), (p + f * 1.3 + r * s * 0.4, y + s * math.radians(55), pt)])]
    # (orbit: around the looked-at point, the camera's distance kept)
    c = hit.copy(); c[2] = p[2]
    a0 = math.atan2(p[1] - c[1], p[0] - c[0]); keys = []
    for da in (-0.45, 0.0, 0.45):
        a = a0 + s * da; q = c + d * np.array([math.cos(a), math.sin(a), 0.0]); q[2] = p[2]
        keys.append((q, math.atan2(c[1] - q[1], c[0] - q[0]), pt))
    out.append(("orbit", keys))
    return out


def sample_keys(keys, frames):
    """Catmull-Rom through key positions, angles eased between keys (unwrapped yaw)"""
    P = [np.asarray(k[0], float) for k in keys]; Y = [keys[0][1]]
    for k in keys[1:]: Y.append(Y[-1] + _wrap(k[1] - Y[-1]))
    A = [(yy, k[2]) for yy, k in zip(Y, keys)]
    seg = len(P) - 1; out = []
    g = lambda L, j: L[max(0, min(len(L) - 1, j))]
    for i in range(frames):
        u = i / (frames - 1) * seg; k = min(int(u), seg - 1); t = u - k
        p0, p1, p2, p3 = g(P, k - 1), g(P, k), g(P, k + 1), g(P, k + 2)
        t2, t3 = t * t, t * t * t
        pos = 0.5 * (2 * p1 + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2 + (-p0 + 3 * p1 - 3 * p2 + p3) * t3)
        te = t * t * (3 - 2 * t)
        out.append((pos.tolist(), g(A, k)[0] + (g(A, k + 1)[0] - g(A, k)[0]) * te, g(A, k)[1] + (g(A, k + 1)[1] - g(A, k)[1]) * te))
    return out


def path_stats(depths, Ks, c2ws, cell=0.25):
    """safety and coverage of a path from its sampled frames' depth: nearest surface (2nd percentile), how much of the frames is close
    (wall-hugging), the distinct 25 cm cells seen, and the point cloud (for the reference choice)"""
    near, close, cells, pts = [], [], set(), []
    for d, K, c in zip(depths, Ks, c2ws):
        hit = d[d > 0]
        if hit.size < 0.5 * d.size: near.append(0.0); close.append(1.0); continue
        near.append(float(np.percentile(hit, 2))); close.append(float((hit < 1.2).mean()))
        pw, _, _ = unproject(d, K, c, stride=2)
        cells.update(map(tuple, np.floor(pw / cell).astype(int).tolist())); pts.append(pw)
    return dict(near=min(near) if near else 0.0, close=float(np.mean(close)) if close else 1.0, cells=len(cells),
                points=np.concatenate(pts) if pts else np.zeros((0, 3)))


def path_score(st, min_depth):
    """0 for an unsafe path; else coverage, less for frames full of near wall"""
    if st["near"] < min_depth: return 0.0
    return st["cells"] * (1.0 - 0.8 * min(1.0, st["close"] / 0.6))


# ------------------------------------------------------------------------------------------------ quality checks
def blocked(depth):
    """a view that one near thing fills: over half the frame nearer than 0.8 m, or nothing past 1 m in the middle"""
    hit = depth[depth > 0]
    if hit.size < 0.5 * depth.size: return "sees-void"
    if (hit < 0.8).mean() > 0.5: return "near-fills-frame"
    if np.median(hit) < 0.9: return "too-close"
    return None


def duplicate(a, b):
    """two views of one era that show the same surfaces from nearly the same place: share of a's points b shows on its surface"""
    da, Ka, ca, Wa, Ha = a; db, Kb, cb, Wb, Hb = b
    pw, _, _ = unproject(da, Ka, ca, stride=8)
    return overlap(pw, Kb, np.linalg.inv(cb), Wb, Hb, depth=db)


# ------------------------------------------------------------------------------------------------ Gaussian-splat readiness
def splat_cells(depths, Ks, c2ws, cell=0.15, stride=1, room_poly=None, pad=0.15):
    """surface cells (15 cm) seen by a set of views -> {cell hash: (frames seeing it, half-angle spread of the viewing directions,
    degrees)}. A splat (or any multi-view reconstruction) needs each surface from several places at clearly different angles"""
    keys_all, dirs_all, frame_all = [], [], []
    for fi, (d, K, C) in enumerate(zip(depths, Ks, c2ws)):
        C = np.asarray(C, float)
        pw, _, _ = unproject(np.where(d > FAR, 0.0, d), K, C, stride)
        if room_poly is not None and len(pw):
            # (the photographed room only: other rooms through a door and the garden through a window are seen from a few places at most)
            from matplotlib.path import Path
            inside = Path(np.asarray(room_poly)).contains_points(pw[:, :2], radius=pad) | Path(np.asarray(room_poly)).contains_points(pw[:, :2], radius=-pad)
            pw = pw[inside]
        if not len(pw): continue
        dv = pw - C[:3, 3]; dv /= np.linalg.norm(dv, axis=1, keepdims=True) + 1e-9
        k = np.floor(pw / cell).astype(np.int64)
        h = (k[:, 0] * 73856093) ^ (k[:, 1] * 19349663) ^ (k[:, 2] * 83492791)
        # (one direction per cell and frame)
        u, inv = np.unique(h, return_inverse=True)
        md = np.zeros((len(u), 3)); np.add.at(md, inv, dv); md /= np.linalg.norm(md, axis=1, keepdims=True) + 1e-9
        keys_all.append(u); dirs_all.append(md); frame_all.append(np.full(len(u), fi))
    if not keys_all: return {}
    h = np.concatenate(keys_all); dv = np.concatenate(dirs_all)
    order = np.argsort(h, kind="stable"); h, dv = h[order], dv[order]
    starts = np.r_[0, np.nonzero(np.diff(h))[0] + 1]; counts = np.diff(np.r_[starts, len(h)])
    mean = np.add.reduceat(dv, starts, axis=0); mean /= np.linalg.norm(mean, axis=1, keepdims=True) + 1e-9
    ang = np.degrees(np.arccos(np.clip(np.sum(dv * np.repeat(mean, counts, axis=0), axis=1), -1, 1)))
    spread = np.maximum.reduceat(ang, starts)
    return dict(zip(h[starts].tolist(), zip(counts.tolist(), spread.tolist())))


def splat_readiness(path_cells, ref_cells, min_views=3, min_spread_deg=15.0):
    """share of the reference surfaces (what the room's camera pool can see) that a walk covers well enough to splat: seen from
    `min_views` frames at least `min_spread_deg` apart (half-spread to the mean direction >= half of it)"""
    ref = set(ref_cells) | set(path_cells)
    good = sum(1 for k, (n, s) in path_cells.items() if n >= min_views and s >= min_spread_deg / 2)
    seen = len(set(path_cells) & ref)
    return dict(readiness=round(good / max(1, len(ref)), 3), seen=round(seen / max(1, len(ref)), 3), cells=len(ref))


def survey_keys(pool, room_poly, rng, step_m=1.0, heights=(1.55, 1.1), sway_deg=20.0, inner=0.0, lead_deg=0.0):
    """a loop around the room through the pool's valid positions (sorted by angle about the room's centre), looking inward with a
    swaying offset, one lap per height: forward motion everywhere, no turning in place"""
    poly = np.asarray(room_poly, float); c = poly.mean(0)
    pts = sorted(pool, key=lambda v: math.atan2(v["pos"][1] - c[1], v["pos"][0] - c[0]))
    keys, last = [], None
    for v in pts:
        p = np.array(v["pos"], float)
        if last is not None and np.linalg.norm(p[:2] - last[:2]) < step_m: continue
        keys.append(p); last = p
    if len(keys) < 3: return []
    out = []
    for li, hgt in enumerate(heights):
        for i, p in enumerate(keys):
            q = p.copy(); q[2] = hgt
            # (inner > 0: the later laps run that share of the way towards the centre -- other viewpoints of the same walls)
            if li and inner: q[:2] = q[:2] + inner * (c - q[:2])
            # (lead_deg: look ahead of the centre along the walking direction, so a surface stays in view over a long stretch of motion)
            to_c = math.atan2(c[1] - q[1], c[0] - q[0]) + math.radians(lead_deg) + math.radians(sway_deg) * (1 if i % 2 else -1)
            out.append((q, to_c, math.radians(-8 if li % 2 == 0 else 2)))
    out.append(out[0])
    return out


# ------------------------------------------------------------------------------------------------ reference photo choice by content
def cell_hash(pw, cell=0.15):
    k = np.floor(pw / cell).astype(np.int64)
    return (k[:, 0] * 73856093) ^ (k[:, 1] * 19349663) ^ (k[:, 2] * 83492791)


def _in_room(pw, room_poly, pad=0.15):
    if room_poly is None or not len(pw): return np.ones(len(pw), bool)
    from matplotlib.path import Path
    P = Path(np.asarray(room_poly))
    return P.contains_points(pw[:, :2], radius=pad) | P.contains_points(pw[:, :2], radius=-pad)


def view_cells(depth, K, c2w, room_poly=None, cell=0.15):
    """{cell hash: unit viewing direction} of the room surfaces one view shows"""
    C = np.asarray(c2w, float)
    pw, _, _ = unproject(np.where(depth > FAR, 0.0, depth), K, C)
    pw = pw[_in_room(pw, room_poly)]
    if not len(pw): return {}
    dv = pw - C[:3, 3]; dv /= np.linalg.norm(dv, axis=1, keepdims=True) + 1e-9
    h = cell_hash(pw, cell); u, inv = np.unique(h, return_inverse=True)
    md = np.zeros((len(u), 3)); np.add.at(md, inv, dv); md /= np.linalg.norm(md, axis=1, keepdims=True) + 1e-9
    return dict(zip(u.tolist(), md))


def changed_cells(depth_a, depth_b, K, c2w, room_poly=None, cell=0.15, tol_abs=0.08, tol_rel=0.05):
    """cells where one camera sees different surfaces in two eras (same pose, depth a vs depth b): something moved, came or went.
    Both eras' surface points there count (the object and what it hid)"""
    a = np.where(depth_a > FAR, 0.0, depth_a); b = np.where(depth_b > FAR, 0.0, depth_b)
    diff = (a > 0) & (b > 0) & (np.abs(a - b) > tol_abs + tol_rel * np.minimum(a, b))
    out = set()
    for d in (a, b):
        dd = np.where(diff, d, 0.0)
        pw, _, _ = unproject(dd, K, np.asarray(c2w, float))
        pw = pw[_in_room(pw, room_poly)]
        if len(pw): out.update(cell_hash(pw, cell).tolist())
    return out


def greedy_refs(cands, n, video_cells, changed, weights=(1.0, 0.5, 1.0), min_angle_deg=15.0):
    """pick n of the candidates' cell maps {hash: dir} one at a time by what each adds: target-video cells not yet seen from 2 directions
    15 deg+ apart (w_video), room cells not yet seen (w_room), changed cells not yet seen (w_change); each term over its total. Returns
    the order picked and each pick's gains"""
    if not cands or n <= 0: return [], []
    wv, wr, wc = weights; cos_t = math.cos(math.radians(min_angle_deg))
    seen_dirs = {}; room_seen = set(); ch_seen = set(); picked, gains = [], []
    nv, nr, nc = max(1, len(video_cells)), max(1, len(set().union(*[c.keys() for c in cands]))), max(1, len(changed))
    for _ in range(min(n, len(cands))):
        best, bg, bt = None, -1.0, None
        for i, c in enumerate(cands):
            if i in picked: continue
            gv = 0
            for h, d in c.items():
                if h not in video_cells: continue
                ds = seen_dirs.get(h)
                if ds is None: gv += 1
                elif len(ds) == 1 and float(ds[0] @ d) < cos_t: gv += 1
            gr = sum(1 for h in c if h not in room_seen)
            gc = sum(1 for h in c if h in changed and h not in ch_seen)
            g = wv * gv / nv + wr * gr / nr + wc * gc / nc
            if g > bg: best, bg, bt = i, g, (gv, gr, gc)
        if best is None: break
        picked.append(best); gains.append(dict(total=round(bg, 4), video=bt[0], room=bt[1], changed=bt[2]))
        for h, d in cands[best].items():
            room_seen.add(h)
            if h in changed: ch_seen.add(h)
            if h in video_cells:
                ds = seen_dirs.setdefault(h, [])
                if len(ds) < 2 and all(float(x @ d) < cos_t for x in ds): ds.append(d)
    return picked, gains


def ref_coverage(ref_maps, video_cells, changed, room_cells, min_angle_deg=15.0):
    """how well a set of reference views covers the target: video cells seen at all and from 2 directions 15 deg+ apart, room cells,
    changed cells (shares)"""
    cos_t = math.cos(math.radians(min_angle_deg)); dirs = {}
    for m in ref_maps:
        for h, d in m.items(): dirs.setdefault(h, []).append(d)
    def multi(ds): return any(float(ds[i] @ ds[j]) < cos_t for i in range(len(ds)) for j in range(i + 1, len(ds)))
    v = [h for h in video_cells if h in dirs]
    return dict(video_seen=round(len(v) / max(1, len(video_cells)), 3), video_multi=round(sum(1 for h in v if multi(dirs[h])) / max(1, len(video_cells)), 3),
                room=round(len(set(dirs) & room_cells) / max(1, len(room_cells)), 3), changed=round(len(set(dirs) & changed) / max(1, len(changed)), 3) if changed else None)

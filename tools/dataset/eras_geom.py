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

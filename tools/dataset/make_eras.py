#!/usr/bin/env python3
"""make_eras.py CONFIG.json OUT_DIR [--only HOME] [--resume] [--dry-run]

Training examples for "predict an indoor video from photos of the same home taken in other eras": per home one house shell (floor plan,
walls, doors, windows fixed) and N eras that drift from one to the next -- furniture re-arranged or replaced, mess, people, repaint,
new floors, lighting / time of day, doors, camera intrinsics -- each with a handful of photos (RGB + metric depth + OpenCV cameras), and one
target video walking through one era. Rendering is the Rust path tracer (tools/preview --rt full for photos, --rt video for the video).

Run with a python that has numpy + Pillow (the Blender venv works):  $BLENDER_PY tools/dataset/make_eras.py cfg.json out/
Config: see DEFAULTS below and tools/dataset/README_eras.md; levels are 0..1 chances that a factor changes from one era to the next."""
import os, sys, json, time, math, copy, random, shutil, subprocess, argparse, zlib, threading
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import eras_geom as G

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
BL2CV = [[1, 0, 0, 0], [0, -1, 0, 0], [0, 0, -1, 0], [0, 0, 0, 1]]

DEFAULTS = {
    "width": 832, "height": 480,             # target video size (Hy-Nomas 480p: multiples of 32); photos keep this pixel area
    "eras": 3,                               # eras per home (era 0 sets the shell); each more era adds ~8-20 s
    "photo_quality": "full",                 # preview --rt full (closer to Blender; benchmark: -3 % examples per hour against fast)
    "photos_per_era": 5,
    "photo_spares": 1,                       # extra views rendered per era for the ones the checks drop (each costs a render)                     # photos of every era but the target's
    "target_photos": 2,                      # photos of the target era itself (0: the model sees the target era only in the video)
    "target": {"era": "random", "count": 2, "path_engine": "cpu", "frames": 64, "fps": 24, "path_views": 3, "min_depth_m": 0.3, "depth": False},  # depth: per-frame depth maps (+~15 % video time)
    # chance that a factor changes from one era to the next (0 = never: every era as era 0; 1 = every era)
    "levels": {"furniture": 0.5, "mess": 0.5, "people": 0.6, "repaint": 0.4, "refloor": 0.1, "lighting": 0.8, "doors": 0.5, "intrinsics": 0.5},
    # a furniture change is one of these kinds (relative weights): move pieces about; re-arrange (the solver's rooms -- living, dining,
    # bedrooms, bathrooms -- laid out again with new pieces on the same house: ~8 s); replace (a whole new furniture solve, which also
    # changes the kitchen passes: ~20 s in a process of its own, off by default); empty (moving-in day: built-ins only)
    "furniture": {"kinds": {"move": 0.3, "rearrange": 0.5, "replace": 0.0, "empty": 0.2}, "jitter_m": 0.35, "jitter_deg": 25.0},
    "mess": {"max_level": 0.9},
    "people": {"max": 4, "rooms": "target"},  # "target": only the photographed room; [] = anywhere; or room types / names
    "intrinsics": {"focal_mm": [22, 45], "focal_spread": 1.15, "aspects": [[4, 3], [3, 2], [16, 9], [1, 1], [3, 4], [2, 3]],
                   "principal_jitter": 0.04},  # principal point off centre by up to this fraction of the image size
    "gen": {"cloth": {"on": False}, "geometry": {"mesh_budget": True}},
    # reference photos picked for what the target video shows: this share of each era's photos among the pool views that see at least
    # `min` of the video's points (frustum estimate); the rest spread over the room
    "ref_overlap": {"share": 0.6, "min": 0.15},
    # quality gate per home (manifest "qc"; a failing home goes to OUT/_rejected): photos per era at least, video seen share at least,
    # video frames close to walls at most, duplicate photo overlap
    "qc": {"min_photos": 3, "min_seen": 0.08, "max_close": 0.45, "duplicate_overlap": 0.7},
    "budget_s": 60,                          # seconds per home: a second target video is rendered only while the home still fits
    "parallel_gen": "auto",                  # gen2 processes at once; auto: from RAM (a whole house peaks near 3 GB) and cores
    "keep_glb": False,
    "homes": [{"seed": 7, "room": "LivingRoom"}],   # or {"count": N, "seed": S, "rooms": [...]}
}


# what an empty era keeps (src/scenegen.rs empty_rooms)
KEEP_BUILTIN = ["Window", "Door", "CeilingLight", "Kitchen", "Fridge", "Dishwasher", "Oven", "Stove", "Hood", "Microwave", "Toilet", "Bathtub", "Sink", "Shower", "Fireplace", "Radiator"]


def crc(*a): return zlib.crc32("/".join(map(str, a)).encode()) & 0x7FFFFFFF


def deep_merge(a, b):
    a = copy.deepcopy(a)
    for k, v in (b or {}).items():
        a[k] = deep_merge(a[k], v) if isinstance(v, dict) and isinstance(a.get(k), dict) else copy.deepcopy(v)
    return a


def find_gen2(cfg):
    for c in (cfg.get("gen2"), os.environ.get("GEN2"), os.path.join(ROOT, "target/fast/gen2"), os.path.join(ROOT, "target/release/gen2")):
        if c and os.path.exists(c): return c
    sys.exit("gen2 binary not found: set GEN2 or config 'gen2'")


def find_preview(cfg):
    for c in (cfg.get("preview"), os.environ.get("PREVIEW"), os.path.join(ROOT, "tools/preview/target/release/preview")):
        if c and os.path.exists(c): return c
    sys.exit("preview binary not found: build tools/preview (cargo build --release) or set PREVIEW")


def room_slug(room):
    return room.lower().replace("room", "-room") if room.endswith("Room") and room != "Room" else room.lower()


def homes_of(cfg):
    h = cfg["homes"]
    if isinstance(h, list): return [dict(x, name=x.get("name") or "%s_%d" % (room_slug(x["room"]), x["seed"])) for x in h]
    r = random.Random(h.get("seed", 0)); rooms = h.get("rooms", ["LivingRoom"])
    out = []
    for i in range(int(h["count"])):
        s = int(h.get("seed", 0)) * 1000 + i; rm = rooms[r.randrange(len(rooms))]
        out.append(dict(seed=s, room=rm, name="%s_%d" % (room_slug(rm), s)))
    return out


# ------------------------------------------------------------------------------------------------ eras
def intrinsics_kit(r, ic, W, H):
    """an era's camera: focal range (36 mm sensor width), aspect / orientation, principal point offset; pixel area of W x H"""
    f0 = math.exp(r.uniform(math.log(ic["focal_mm"][0]), math.log(ic["focal_mm"][1])))
    a = ic["aspects"][r.randrange(len(ic["aspects"]))]; ar = a[0] / a[1]
    area = W * H; w = int(round(math.sqrt(area * ar) / 16)) * 16; h = int(round(area / w / 16)) * 16
    pj = ic.get("principal_jitter", 0.0)
    return dict(focal_mm=[round(f0, 2), round(f0 * ic.get("focal_spread", 1.15), 2)], aspect=a, width=w, height=h,
                principal_offset=[round(r.uniform(-pj, pj), 4), round(r.uniform(-pj, pj), 4)])


def plan_eras(home, cfg):
    """era k drifts from era k-1: every factor changes with its level's chance. Returns [{era, state, changed}]"""
    lv, n = cfg["levels"], int(cfg["eras"]); hs = home["seed"]
    r = random.Random(crc(hs, "eras"))
    W, H = cfg["width"], cfg["height"]
    def people_n(): return r.randint(1, cfg["people"]["max"]) if r.random() < lv["people"] else 0
    st = dict(furniture=dict(seed=hs, jitter=None, rearrange=None, empty=False), mess=dict(level=round(r.uniform(0, cfg["mess"]["max_level"]) * lv["mess"], 3), seed=crc(hs, 0, "mess")),
              people=dict(count=people_n(), seed=crc(hs, 0, "people")), walls=crc(hs, 0, "walls"), floor=None, lighting=crc(hs, 0, "lighting"),
              doors=crc(hs, 0, "doors"), intrinsics=intrinsics_kit(r, cfg["intrinsics"], W, H))
    eras = [dict(era=0, state=copy.deepcopy(st), changed=[])]
    fc = cfg["furniture"]
    for k in range(1, n):
        ch = []
        if r.random() < lv["furniture"]:
            kinds = [(k_, w) for k_, w in fc["kinds"].items() if w > 0 and not (k_ == "empty" and st["furniture"]["empty"])]
            x = r.random() * sum(w for _, w in kinds); kind = kinds[-1][0]
            for k_, w in kinds:
                if x < w: kind = k_; break
                x -= w
            f = st["furniture"]; sd = f["seed"]
            if kind == "move": st["furniture"] = dict(f, jitter=dict(seed=crc(hs, k, "jitter"), translate_m=fc["jitter_m"], rotate_deg=fc["jitter_deg"]), empty=False)
            elif kind == "rearrange": st["furniture"] = dict(seed=sd, jitter=None, rearrange=crc(hs, k, "rearrange"), empty=False)
            elif kind == "replace": st["furniture"] = dict(seed=crc(hs, k, "furniture"), jitter=None, rearrange=None, empty=False)
            else: st["furniture"] = dict(f, jitter=None, empty=True)
            ch.append("furniture:" + kind)
        if r.random() < lv["mess"]: st["mess"] = dict(level=round(r.uniform(0, cfg["mess"]["max_level"]), 3), seed=crc(hs, k, "mess")); ch.append("mess")
        # (people move between any two photos; whether anyone is there is what the level decides)
        st["people"] = dict(count=people_n(), seed=crc(hs, k, "people"))
        if st["people"]["count"]: ch.append("people")
        if r.random() < lv["repaint"]: st["walls"] = crc(hs, k, "walls"); ch.append("repaint")
        if r.random() < lv["refloor"]: st["floor"] = crc(hs, k, "floor"); ch.append("refloor")
        if r.random() < lv["lighting"]: st["lighting"] = crc(hs, k, "lighting"); ch.append("lighting")
        if r.random() < lv["doors"]: st["doors"] = crc(hs, k, "doors"); ch.append("doors")
        if r.random() < lv["intrinsics"]: st["intrinsics"] = intrinsics_kit(r, cfg["intrinsics"], W, H); ch.append("intrinsics")
        eras.append(dict(era=k, state=copy.deepcopy(st), changed=ch))
    # target eras: `target.count` of them (each gets its own video; the other eras are its references)
    t = cfg["target"]["era"]; c = max(1, min(n - 1, int(cfg["target"].get("count", 1))))
    if t == "random": tgts = r.sample(range(n), c)
    elif t == "last": tgts = list(range(n - 1, n - 1 - c, -1))
    else: tgts = [int(t)]
    return eras, tgts


def gen_config(home, cfg, st, shell_json):
    g = deep_merge(cfg["gen"], {
        "house": {"seed": home["seed"], "room": home["room"], "only_target": False},
        "cameras": {"count": 0},  # (the sidecar preview cameras: the dataset pool replaces them)
        "variation": {"doors": {"mode": "random", "seed": st["doors"]}, "walls": {"mode": "interior", "seed": st["walls"]},
                      "lighting": {"mode": "random", "lamps": "auto", "seed": st["lighting"]},
                      "furniture": {"seed": st["furniture"]["seed"], **({"jitter": st["furniture"]["jitter"]} if st["furniture"]["jitter"] else {}),
                                    **({"rearrange": st["furniture"]["rearrange"]} if st["furniture"].get("rearrange") else {}), "empty": bool(st["furniture"].get("empty"))}},
        "mess": {"level": 0.0 if st["furniture"].get("empty") else st["mess"]["level"], "seed": st["mess"]["seed"], "floor_scale": 0.27, "surface_scale": 4.0},
        "people": {"count": st["people"]["count"], "seed": st["people"]["seed"]},
    })
    if st["floor"]: g["variation"]["floor"] = {"mode": "interior", "seed": st["floor"]}
    pr = cfg["people"]["rooms"]
    g["people"]["rooms"] = [room_slug(home["room"])] if pr == "target" else list(pr)
    # (clutter where the photos are: the other rooms' mess is never seen and cost seconds per era)
    g["mess"]["rooms"] = [room_slug(home["room"])]
    if shell_json: g["house"]["pin_windows"] = shell_json
    return g


# ------------------------------------------------------------------------------------------------ cameras
def c2w_cv(pos, yaw, pitch):
    """OpenCV camera-to-world (x right, y down, z forward), world z up (pool convention, dscap.cam_pose_cv)"""
    f = [math.cos(pitch) * math.cos(yaw), math.cos(pitch) * math.sin(yaw), math.sin(pitch)]
    rt = [f[1], -f[0], 0.0]; n = math.hypot(rt[0], rt[1]) or 1.0; rt = [rt[0] / n, rt[1] / n, 0.0]
    dn = [f[1] * rt[2] - f[2] * rt[1], f[2] * rt[0] - f[0] * rt[2], f[0] * rt[1] - f[1] * rt[0]]
    return [[rt[0], dn[0], f[0], pos[0]], [rt[1], dn[1], f[1], pos[1]], [rt[2], dn[2], f[2], pos[2]], [0, 0, 0, 1]]


def matmul(a, b): return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def inv_rigid(m):
    R = [[m[j][i] for j in range(3)] for i in range(3)]; t = [m[i][3] for i in range(3)]
    ti = [-sum(R[i][k] * t[k] for k in range(3)) for i in range(3)]
    return [R[0] + [ti[0]], R[1] + [ti[1]], R[2] + [ti[2]], [0, 0, 0, 1]]


def cam_json(path, c2w, f_mm):
    """tools/preview --cam: Blender camera matrix (camera looks down -z, y up) + lens on a 36 mm sensor width"""
    json.dump(dict(matrix=matmul(c2w, BL2CV), lens=f_mm, sensor_width=36.0), open(path, "w"))


def view_record(i, c2w, f_mm, W, H, crop=None, Wr=None, Hr=None):
    """per-view camera record; crop = (ox, oy) of a W x H window cut from a Wr x Hr render (principal point off centre)"""
    Wr, Hr = Wr or W, Hr or H; fpx = f_mm / 36.0 * Wr; ox, oy = crop or (0, 0)
    K = [[fpx, 0, Wr / 2 - ox], [0, fpx, Hr / 2 - oy], [0, 0, 1]]
    return dict(index=i, width=W, height=H, K=K, focal_mm=f_mm, sensor_width_mm=36.0 * W / Wr, c2w=c2w, w2c=inv_rigid(c2w),
                K_normalized=[[K[0][0] / W, 0, K[0][2] / W], [0, K[1][1] / H, K[1][2] / H], [0, 0, 1]])


def catmull(p0, p1, p2, p3, t):
    t2, t3 = t * t, t * t * t
    return [0.5 * (2 * p1[i] + (-p0[i] + p2[i]) * t + (2 * p0[i] - 5 * p1[i] + 4 * p2[i] - p3[i]) * t2 + (-p0[i] + 3 * p1[i] - 3 * p2[i] + p3[i]) * t3) for i in range(len(p1))]


def video_paths(pool, n_views, rng):
    """candidate walks: chains of pool views that follow each other (a cluster's trajectory / overlap steps), each step < 2.6 m and < 70 deg"""
    out = []
    for s in range(len(pool)):
        ch = [pool[s]]
        for v in pool[s + 1:]:
            if len(ch) >= n_views: break
            p = ch[-1]; d = math.dist(p["pos"], v["pos"]); dy = abs((v["yaw"] - p["yaw"] + math.pi) % (2 * math.pi) - math.pi)
            if 0.4 < d < 2.6 and dy < math.radians(70): ch.append(v)
        if len(ch) >= 2: out.append(ch)
    rng.shuffle(out); out.sort(key=lambda c: -len(c))
    return out


def sample_path(chain, frames):
    """positions by Catmull-Rom through the chain, yaw / pitch unwrapped and eased, one focal length (the first view's)"""
    P = [c["pos"] for c in chain]; yaws = [chain[0]["yaw"]]
    for c in chain[1:]: yaws.append(yaws[-1] + ((c["yaw"] - yaws[-1] + math.pi) % (2 * math.pi) - math.pi))
    A = [[y, c["pitch"]] for y, c in zip(yaws, chain)]
    seg = len(P) - 1; out = []
    for i in range(frames):
        u = i / (frames - 1) * seg; k = min(int(u), seg - 1); t = u - k
        g = lambda L, j: L[max(0, min(len(L) - 1, j))]
        pos = catmull(g(P, k - 1), g(P, k), g(P, k + 1), g(P, k + 2), t)
        te = t * t * (3 - 2 * t)  # (ease the turn inside each step)
        ang = [g(A, k)[j] + (g(A, k + 1)[j] - g(A, k)[j]) * te for j in range(2)]
        out.append((pos, ang[0], ang[1]))
    return out, chain[0]["f_mm"]


# ------------------------------------------------------------------------------------------------ rendering
def run(cmd, log, env=None):
    t = time.time()
    with open(log, "a") as f:
        f.write("$ " + " ".join(map(str, cmd)) + "\n"); f.flush()
        p = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, env=env)
    return p.returncode, time.time() - t


def render_views(preview, glb, cams, res, quality, out_dir, log, extra=(), depth=True):
    """cams: [(name, c2w, f_mm)] -> pngs + depth npys in out_dir (named <stem>_<name>.png / .depth.npy)"""
    cd = os.path.join(out_dir, "_cams"); os.makedirs(cd, exist_ok=True)
    args = []
    for name, c2w, f in cams:
        p = os.path.join(cd, name + ".json"); cam_json(p, c2w, f); args += ["--cam", p]
    cmd = [preview, glb] + args + ["--res", "%dx%d" % res] + (["--depth"] if depth else []) + ["--sidecar-lighting"] + (["--rt", quality] if quality else ["--ss", "2"]) + list(extra) + ["-o", out_dir]
    return run(cmd, log)


def spread(pool, n):
    """n views far apart (farthest-point order over position, heading and look-at point): a family's photos of a room are not one burst"""
    if not pool: return []
    def d(a, b):
        dy = abs((a["yaw"] - b["yaw"] + math.pi) % (2 * math.pi) - math.pi)
        return math.dist(a["pos"][:2], b["pos"][:2]) + 1.5 * dy + 0.5 * math.dist(a.get("hit", a["pos"])[:2], b.get("hit", b["pos"])[:2])
    out = [pool[0]]; rest = pool[1:]
    while rest and len(out) < n:
        j = max(range(len(rest)), key=lambda i: min(d(rest[i], o) for o in out)); out.append(rest.pop(j))
    return out


def photos_for_era(a, cfg, e, home, era_dir, glb, pool, n, log, vpts=None):
    """n photos of this era from its own camera pool (valid for its furniture), in its own camera kit. With the target video's points
    (vpts), a share of the photos (ref_overlap.share) is picked among the pool views that look at what the video shows, the rest spread
    over the room. Rendered with one spare; a view that is blocked or repeats an earlier photo of this era is dropped (quality gate).
    Returns (view records, dropped, seconds, [(depth, K, c2w, W, H)] of the kept photos)"""
    import numpy as np
    from PIL import Image
    kit = e["state"]["intrinsics"]; W, H = kit["width"], kit["height"]
    # principal point: render with a margin and cut an off-centre window
    ox = int(round(kit["principal_offset"][0] * W)); oy = int(round(kit["principal_offset"][1] * H))
    mx, my = abs(ox), abs(oy); Wr, Hr = W + 2 * mx, H + 2 * my
    ro = cfg["ref_overlap"]; est = {}
    # vpts: one point set per target video; the overlap picks take turns between them, so each video gets photos that look at it
    sets = [p for p in (vpts or []) if p is not None and len(p)]
    if sets and n:
        per = []
        for p in sets:
            e = {}
            for i, v in enumerate(pool):
                K = G.K_of(v["f_mm"], W, H); w2c = np.linalg.inv(np.array(c2w_cv(v["pos"], v["yaw"], v["pitch"])))
                e[i] = G.overlap(p, K, w2c, W, H)
            per.append(e)
        est = {i: max(e[i] for e in per) for i in range(len(pool))}
        n_hi = int(round(ro["share"] * n)); hi = []
        order_ = [sorted(e, key=lambda i: -e[i]) for e in per]; pos_ = [0] * len(per); t_ = 0; stuck = 0
        while len(hi) < n_hi and stuck < len(per):
            e, o = per[t_ % len(per)], order_[t_ % len(per)]; got = False
            while pos_[t_ % len(per)] < len(o):
                i = o[pos_[t_ % len(per)]]; pos_[t_ % len(per)] += 1
                if e[i] < ro["min"]: pos_[t_ % len(per)] = len(o); break
                if i not in hi and all(math.dist(pool[i]["pos"], pool[j]["pos"]) > 0.5 or abs(G._wrap(pool[i]["yaw"] - pool[j]["yaw"])) > math.radians(20) for j in hi):
                    hi.append(i); got = True; break
            stuck = 0 if got else stuck + 1; t_ += 1
        rest = [v for i, v in enumerate(pool) if i not in hi]
        cand = [pool[i] for i in hi] + spread(rest, n + int(cfg["photo_spares"]) - len(hi))
    else:
        cand = spread(pool, n + int(cfg["photo_spares"]))  # (spares for views the checks turn down)
    stem = os.path.splitext(os.path.basename(glb))[0]
    for d in ("photos", "depth"): os.makedirs(os.path.join(era_dir, d), exist_ok=True)
    views, dropped, kept = [], [], []; dt = 0.0; tmp = os.path.join(era_dir, "_r")
    # (a second pass only when the checks left the era short of qc.min_photos: small rooms repeat themselves)
    for pass_ in range(2):
        if pass_:
            need = min(n, int(cfg["qc"]["min_photos"])) - len(views)
            if need <= 0: break
            rest = [v for v in pool if not any(v is c for c in cand)]
            if not rest: break
            cand = spread(rest, need + 1)
        shutil.rmtree(tmp, ignore_errors=True); os.makedirs(tmp)
        # (the pool's focal length is for the W x H frame: the wider render keeps it per pixel)
        cams = [("p%d_%02d" % (pass_, i), c2w_cv(v["pos"], v["yaw"], v["pitch"]), v["f_mm"] * Wr / W) for i, v in enumerate(cand)]
        if not cams: break
        rc, dt_ = render_views(a.preview, glb, cams, (Wr, Hr), a.photo_quality, tmp, log); dt += dt_
        if rc: raise RuntimeError("photo render failed, see %s" % log)
        for i, (nm, c2w, f) in enumerate(cams):
            if len(views) >= n: break
            im = Image.open(os.path.join(tmp, "%s_%s.png" % (stem, nm))).convert("RGB"); d = np.load(os.path.join(tmp, "%s_%s.depth.npy" % (stem, nm)))
            x0, y0 = mx + ox, my + oy
            im = im.crop((x0, y0, x0 + W, y0 + H)); d = d[y0:y0 + H, x0:x0 + W]
            rec = view_record(len(views), c2w, f, W, H, (x0, y0), Wr, Hr); K = np.array(rec["K"]); C = np.array(c2w)
            why = G.blocked(d)
            if not why:
                # (a duplicate shows the same surfaces from nearly the same place: another viewpoint of the same corner is wanted)
                near = [kv for kv in kept if np.linalg.norm(kv[2][:3, 3] - C[:3, 3]) < 0.6 and float(kv[2][:3, 2] @ C[:3, 2]) > math.cos(math.radians(25))]
                dup = max((G.duplicate((d, K, C, W, H), kv) for kv in near), default=0.0)
                if dup > cfg["qc"]["duplicate_overlap"]: why = "duplicate (%.0f %% of it in an earlier photo from nearly the same place)" % (100 * dup)
            if why: dropped.append(dict(pool_index=pool.index(cand[i]), reason=why)); continue
            k = len(views); im.save(os.path.join(era_dir, "photos", "%03d.png" % k)); np.save(os.path.join(era_dir, "depth", "%03d.npy" % k), d.astype(np.float32))
            pi = pool.index(cand[i])
            rec.update(file="photos/%03d.png" % k, depth="depth/%03d.npy" % k, kind=cand[i].get("kind"), pool_index=pi, render_pass=pass_)
            if est: rec["video_overlap_est"] = round(est.get(pi, 0.0), 3)
            views.append(rec); kept.append((d, K, C, W, H))
    shutil.rmtree(tmp, ignore_errors=True)
    return views, dropped, dt, kept


def choose_path(a, cfg, home, tdir, glb, pool, log, cpu_paths=None):
    """the target walk: candidates (pool chains and camera moves from the best pool views: push, pull, pan, strafe, orbit, walk-turn)
    rendered as plain raster depth at 1/8 size, every 8th frame, all in one preview call; the safest widest-seeing one wins (score:
    distinct 25 cm cells seen, less for frames full of near wall; zero when anything comes nearer than min_depth_m).
    -> (path [(pos, yaw, pitch)], focal mm, info, video points)"""
    import numpy as np
    tc = cfg["target"]; W, H, F = cfg["width"], cfg["height"], int(tc["frames"])
    if cpu_paths:
        # (gen2 chose it from the scene's BVH while it built the era: crates/ig_cams/src/pool.rs video_paths)
        b = cpu_paths[0]
        keys = [(np.array(k[0], float), k[1], k[2]) for k in b["keys"]]
        info = dict(family=b["family"], score=round(b["score"], 1), near_m=round(b["near_m"], 2), close_share=round(b["close_share"], 3), cells=b["cells"], engine="cpu",
                    tried=[dict(family=c["family"], score=round(c["score"], 1)) for c in cpu_paths], choose_s=0.0)
        return G.sample_keys(keys, F), b["f_mm"], info, np.array(b["points"], float).reshape(-1, 3)
    rng = random.Random(crc(home["seed"], "video")); nprng = np.random.default_rng(crc(home["seed"], "video-np"))
    stem = os.path.splitext(os.path.basename(glb))[0]; t0 = time.time()
    cands = []
    for chain in video_paths(pool, int(tc["path_views"]), rng)[:2]:
        p, f = sample_path(chain, F); cands.append(("pool-chain", p, f))
    for v in spread(pool, 2):
        for fam, keys in G.move_paths(v, nprng): cands.append((fam, G.sample_keys(keys, F), v["f_mm"]))
    step = max(1, F // 8); w, h = W // 8, H // 8
    cams = [("c%02d_%04d" % (ci, i), c2w_cv(p, y, pi), f) for ci, (_, path, f) in enumerate(cands) for i, (p, y, pi) in enumerate(path) if i % step == 0]
    chk = os.path.join(tdir, "_chk"); shutil.rmtree(chk, ignore_errors=True)
    rc, _ = render_views(a.preview, glb, cams, (w, h), None, chk, log, extra=("--ss", "1"))
    if rc: raise RuntimeError("path check render failed, see %s" % log)
    scored = []
    for ci, (fam, path, f) in enumerate(cands):
        names = [n for n, _, _ in cams if n.startswith("c%02d_" % ci)]
        ds = [np.load(os.path.join(chk, "%s_%s.depth.npy" % (stem, n))) for n in names]
        Cs = [np.array(c) for n, c, _ in cams if n.startswith("c%02d_" % ci)]
        st = G.path_stats(ds, [G.K_of(f, w, h)] * len(ds), Cs)
        sc = G.path_score(st, tc["min_depth_m"]) * rng.uniform(0.85, 1.15)  # (a little chance, so the families mix over a dataset)
        scored.append((sc, ci, st))
    shutil.rmtree(chk, ignore_errors=True)
    if not scored: raise RuntimeError("no video path candidates (camera pool of %d views)" % len(pool))
    sc, ci, st = max(scored, key=lambda x: x[0])
    tried = [dict(family=cands[c][0], score=round(s, 1), near=round(t["near"], 2), close=round(t["close"], 2), cells=t["cells"]) for s, c, t in scored]
    if sc <= 0: raise RuntimeError("no safe video path (tried %s)" % tried)
    pts = st["points"]
    if len(pts) > 20000: pts = pts[nprng.choice(len(pts), 20000, replace=False)]
    info = dict(family=cands[ci][0], score=round(sc, 1), near_m=round(st["near"], 2), close_share=round(st["close"], 3), cells=st["cells"], tried=tried, choose_s=round(time.time() - t0, 1))
    return cands[ci][1], cands[ci][2], info, pts


def render_video(a, cfg, tdir, glb, path, f, log):
    """--rt video along the chosen path; frames + per-frame cameras + mp4 (background encode); the per-frame depth is kept at 1/4 size
    in memory for the masks (and at full size on disk with target.depth)"""
    import numpy as np
    tc = cfg["target"]; W, H = cfg["width"], cfg["height"]
    stem = os.path.splitext(os.path.basename(glb))[0]
    frames_dir = os.path.join(tdir, "_r"); shutil.rmtree(frames_dir, ignore_errors=True)
    cams = [("f%04d" % i, c2w_cv(p, y, pi), f) for i, (p, y, pi) in enumerate(path)]
    rc, dt = render_views(a.preview, glb, cams, (W, H), "video", frames_dir, log, depth=True)
    if rc: raise RuntimeError("video render failed, see %s" % log)
    for d in ("frames",) + (("depth",) if tc.get("depth") else ()): os.makedirs(os.path.join(tdir, d), exist_ok=True)
    recs, small = [], []
    for i, (nm, c2w, _) in enumerate(cams):
        os.replace(os.path.join(frames_dir, "%s_%s.png" % (stem, nm)), os.path.join(tdir, "frames", "%04d.png" % i))
        dp = os.path.join(frames_dir, "%s_%s.depth.npy" % (stem, nm)); d = np.load(dp)
        small.append(d[2::4, 2::4].copy())  # (the pixel centres of a 1/4 grid)
        if tc.get("depth"): os.replace(dp, os.path.join(tdir, "depth", "%04d.npy" % i))
        r = view_record(i, c2w, f, W, H); r.update(file="frames/%04d.png" % i, depth="depth/%04d.npy" % i if tc.get("depth") else None); recs.append(r)
    shutil.rmtree(frames_dir, ignore_errors=True)
    enc = None
    if shutil.which("ffmpeg"):
        # (in the background: the next era's photos render meanwhile; main() waits for it before the home is done)
        enc = subprocess.Popen(["ffmpeg", "-y", "-loglevel", "error", "-framerate", str(tc["fps"]), "-i", os.path.join(tdir, "frames", "%04d.png"), "-c:v", "libx264", "-crf", "14",
                                "-pix_fmt", "yuvj420p", "-color_range", "pc", os.path.join(tdir, "video.mp4")], stdout=subprocess.DEVNULL, stderr=open(log, "a"))
    return dict(fps=tc["fps"], frames=recs, render_s=dt, _enc=enc, _small=small)


def video_labels(tdir, target, refs_by_era, tgt):
    """per-frame masks of the target video against the reference photos of the other eras (eras_geom classes, PNG at 1/4 size) and,
    per reference photo, the share of the video's surface it shows. -> summary"""
    import numpy as np
    from PIL import Image
    os.makedirs(os.path.join(tdir, "masks"), exist_ok=True)
    refs = [(d, K, np.linalg.inv(C)) for k, kv in refs_by_era.items() if k != tgt for (d, K, C, W, H) in kv]
    counts = np.zeros(4); pts = []
    for i, (rec, d) in enumerate(zip(target["frames"], target["_small"])):
        # (small pixel u's centre u + 0.5 is frame pixel 4u + 2's centre 4u + 2.5: cx' = (cx - 0.5) / 4)
        K = np.array(rec["K"], float) / 4.0; K[2, 2] = 1.0; K[0, 2] = (rec["K"][0][2] - 0.5) / 4.0; K[1, 2] = (rec["K"][1][2] - 0.5) / 4.0
        m = G.frame_mask(d, K, np.array(rec["c2w"]), refs)
        Image.fromarray(m).save(os.path.join(tdir, "masks", "%04d.png" % i))
        surf = (d > 0) & (m != G.OUTSIDE); counts += np.bincount(m[surf], minlength=4)[:4]
        if i % 8 == 0: pts.append(G.unproject(np.where(d > G.FAR, 0.0, d), K, np.array(rec["c2w"]), stride=2)[0])
        rec["mask"] = "masks/%04d.png" % i
    vp = np.concatenate(pts) if pts else np.zeros((0, 3))
    per_photo = {k: [round(G.overlap(vp, K, np.linalg.inv(C), W, H, depth=d), 3) for (d, K, C, W, H) in kv] for k, kv in refs_by_era.items()}
    tot = counts.sum() or 1.0
    return dict(legend=dict({str(k): v for k, v in G.MASK_LEGEND.items()}, **{str(G.OUTSIDE): "past %g m: the outdoors through a window (not in the shares)" % G.FAR}), size="1/4 of the frame, pixel (i, j) = frame pixel (4i+2, 4j+2)",
                share={name: round(float(counts[c] / tot), 3) for c, name in ((G.SEEN, "seen"), (G.CHANGED, "changed"), (G.HIDDEN, "hidden"), (G.NEVER, "never"))},
                per_photo=per_photo)


# ------------------------------------------------------------------------------------------------ sheet
def contact_sheet(home_dir, man, out_png):
    from PIL import Image, ImageDraw
    th = 150; rows = []
    for e in man["eras"]:
        ims = [Image.open(os.path.join(home_dir, e["dir"], v["file"])).convert("RGB") for v in e["photos"]]
        ims = [im.resize((max(1, int(im.width * th / im.height)), th)) for im in ims]
        tg = {t["era"] for t in man["targets"]}
        lab = "era %d%s: %s" % (e["era"], " (target)" if e["era"] in tg else "", ", ".join(e["changed"]) or "-")
        rows.append((lab, ims))
    for t in man["targets"]:
        fr = t["frames"]; pick = [fr[int(i * (len(fr) - 1) / 5)] for i in range(6)]
        sh_ = t["masks"]["share"]
        rows.append(("target video era %d (%s path; references show %.0f %%, changed %.0f %%, never seen %.0f %%)" % (t["era"], t["video_path"]["family"], 100 * sh_["seen"], 100 * sh_["changed"], 100 * sh_["never"]),
                     [Image.open(os.path.join(home_dir, t["dir"], v["file"])).convert("RGB").resize((int(th * v["width"] / v["height"]), th)) for v in pick]))
    W = max(sum(im.width + 4 for im in ims) for _, ims in rows) + 4; Hh = len(rows) * (th + 22) + 4
    sh = Image.new("RGB", (W, Hh), (24, 24, 24)); dr = ImageDraw.Draw(sh); y = 4
    for lab, ims in rows:
        dr.text((6, y), lab, fill=(230, 230, 230)); x = 4; y += 18
        for im in ims: sh.paste(im, (x, y)); x += im.width + 4
        y += th + 4
    sh.save(out_png)


# ------------------------------------------------------------------------------------------------ main
def main():
    ap = argparse.ArgumentParser(); ap.add_argument("config"); ap.add_argument("out")
    ap.add_argument("--only"); ap.add_argument("--resume", action="store_true"); ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    cfg = deep_merge(DEFAULTS, json.load(open(a.config)))
    out = os.path.abspath(a.out); os.makedirs(out, exist_ok=True)
    a.gen2, a.preview = find_gen2(cfg), find_preview(cfg); a.photo_quality = cfg["photo_quality"]
    if cfg["parallel_gen"] == "auto":
        try: ram = int(subprocess.run(["sysctl", "-n", "hw.memsize"], capture_output=True, text=True).stdout) / 1e9
        except Exception: ram = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES") / 1e9
        # (the renderer keeps ~1.5 GB; past the RAM the gens swap and three ran slower than one after another)
        cfg["parallel_gen"] = max(1, min((os.cpu_count() or 2) // 2, int((ram - 1.5) / 3.0)))
    print("gen2 %s, preview %s, %d generations at once" % (a.gen2, a.preview, cfg["parallel_gen"]), flush=True)
    homes = [h for h in homes_of(cfg) if not a.only or h["name"] == a.only]
    plans = [(h,) + plan_eras(h, cfg) for h in homes]
    if a.dry_run:
        for h, eras, tgts in plans:
            print("== %s target eras %s" % (h["name"], tgts))
            for e in eras: print("  era %d changed %s\n    %s" % (e["era"], e["changed"], json.dumps(gen_config(h, cfg, e["state"], "SHELL" if e["era"] else None))))
        return
    W, H = cfg["width"], cfg["height"]
    # generation: one gen2 --batch process per (home, furniture seed). Eras that share the furniture seed run in one process one after
    # another and reuse its solved layout (moved, re-arranged, emptied, repainted ... eras cost a scene build, not a solve)
    jobs = {}     # (home index, group key) -> (Popen, log file, t0)
    started = set()

    def paths(hi, k):
        h = plans[hi][0]; hd = os.path.join(out, h["name"]); ed = os.path.join(hd, "era_%d" % k)
        return hd, ed, os.path.join(hd, "_gen", "era_%d.glb" % k)

    def groups(hi):
        """[(key, [eras], independent)]: the batches of a home. Era 0's furniture seed is split over parallel_gen batches that all start at
        once: same settings, same solve, so the same layout and windows without pinning (independent). Eras of other furniture seeds
        (replaced furniture) pin era 0's windows and wait for it. The target era goes first in its batch: its video, the longest render,
        then runs while the other eras are built"""
        h, eras, tgts = plans[hi]; gs = {}; tgt = tgts[0]
        for e in eras: gs.setdefault(e["state"]["furniture"]["seed"], []).append(e["era"])
        g0 = eras[0]["state"]["furniture"]["seed"]; first = gs.pop(g0)
        for ks in gs.values():
            ks.sort(key=lambda k: (k not in tgts, k))
        # (a second batch solves the same layout again, and the two slow each other down: worth it only for a long group)
        nb = max(1, min(int(cfg["parallel_gen"]), len(first) // 2)) if len(first) > 3 else 1
        # (the lead era -- the target when it shares era 0's furniture, so its video starts after one build -- defines the shell)
        lead = tgt if tgt in first else 0
        rest = sorted([k for k in first if k != lead], key=lambda k: (k not in tgts, k))  # (the other targets next: their videos queue early)
        bs = [[lead]] + [[] for _ in range(nb - 1)]
        for i, k in enumerate(rest): bs[(i + 1) % nb if nb > 1 else 0].append(k)
        out = [(g0, b, True) for b in bs if b]
        return out + [(k, ks, False) for k, ks in sorted(gs.items(), key=lambda x: (tgt not in x[1], x[1][0]))]

    def lead(hi): return groups(hi)[0][1][0]

    def entry(hi, k, pin):
        h, eras, tgts = plans[hi]; hd, ed, glb = paths(hi, k); tgt = k if k in tgts else -1; os.makedirs(os.path.dirname(glb), exist_ok=True); os.makedirs(ed, exist_ok=True)
        shell = os.path.join(hd, "_gen", "era_%d.house.json" % lead(hi)) if pin else None
        g = gen_config(h, cfg, eras[k]["state"], shell); cp = os.path.join(hd, "_gen", "era_%d.cfg.json" % k); json.dump(g, open(cp, "w"), indent=1)
        # the era's own camera pool: valid views for its furniture, in its camera kit
        n = (int(cfg["target_photos"]) if k == tgt else int(cfg["photos_per_era"])) + 1
        if k == tgt: n = max(n, 8)
        kit = eras[k]["state"]["intrinsics"]
        sp = dict(width=kit["width"], height=kit["height"], n_views=n, camera_seed=crc(h["seed"], k, "cams"),
                  cameras=dict(focal_mm=kit["focal_mm"], eye_height_m=[1.0, 1.75], traj_frac=0.5, cluster_len=4, pool_factor=3 if k == tgt else 2),
                  pool_path=os.path.join(hd, "_gen", "era_%d.pool.json" % k), room_slug=room_slug(h["room"]),
                  aim_file=os.path.join(hd, "_gen", "aim_points.json"), aim_share=1.0)
        if k in tgts and cfg["target"].get("path_engine") == "cpu":
            tc = cfg["target"]
            sp["video"] = dict(frames=int(tc["frames"]), width=W, height=H, min_depth_m=tc["min_depth_m"], path_views=tc["path_views"], starts=8)
        spp = os.path.join(hd, "_gen", "era_%d.dspool.json" % k); json.dump(sp, open(spp, "w"))
        return dict(args=[str(h["seed"]), h["room"], glb, "--config", cp], env=dict(DSPOOL=spp, LENS_MIN=str(kit["focal_mm"][0])),
                    done=os.path.join(hd, "_gen", "era_%d.done" % k))

    def start_job(hi, gi):
        key = (hi, gi)
        if key in started: return
        hd = paths(hi, 0)[0]; gk, ks, indep = groups(hi)[gi]
        bp = os.path.join(hd, "_gen", "batch_%d.json" % gi); json.dump([entry(hi, k, not indep) for k in ks], open(bp, "w"), indent=1)
        lf = open(os.path.join(hd, "_gen", "batch_%d.log" % gi), "w")
        jobs[key] = (subprocess.Popen([a.gen2, "--batch", bp], stdout=lf, stderr=subprocess.STDOUT), lf, time.time()); started.add(key)

    def running(): return sum(1 for (p, _, _) in jobs.values() if p.poll() is None)

    def fill(hi):
        """keep up to parallel_gen batches running: this home's groups (the later ones wait for era 0's windows), then the next home's first"""
        for hj in (hi, hi + 1):
            if hj >= len(plans): return
            hd = paths(hj, 0)[0]
            ready = os.path.exists(os.path.join(hd, "_gen", "era_%d.done" % lead(hj)))
            for gi, (_, _, indep) in enumerate(groups(hj)):
                if running() >= cfg["parallel_gen"]: return
                if (hj, gi) in started: continue
                if indep or ready: start_job(hj, gi)
            if not ready: return   # (one home ahead at most, and only once this one's era 0 is out)

    def wait_era(hi, k, deadline=None):
        """block until era k's build is written (fill() keeps the other batches going); -> (ok, seconds of its build); past `deadline`
        (time.time()) -> (None, 0): the era comes too late for the home's budget"""
        hd = paths(hi, 0)[0]; mk = os.path.join(hd, "_gen", "era_%d.done" % k)
        gi = next(i for i, (_, ks, _) in enumerate(groups(hi)) if k in ks)
        while True:
            fill(hi)
            if (hi, gi) not in started: start_job(hi, gi)
            if os.path.exists(mk): return True, float(open(mk).read() or 0)
            p = jobs[(hi, gi)][0]
            if p.poll() is not None and not os.path.exists(mk): return False, 0.0
            if deadline is not None and time.time() > deadline: return None, 0.0
            time.sleep(0.2)

    def build_home(hi, h, eras, tgts):
        hd = os.path.join(out, h["name"])
        if a.resume and os.path.exists(os.path.join(hd, "manifest.json")): print("skip", h["name"]); return
        t_home = time.time(); log = os.path.join(hd, "render.log"); os.makedirs(hd, exist_ok=True)
        print("== %s (%s, seed %d): %d eras, target eras %s" % (h["name"], h["room"], h["seed"], len(eras), tgts), flush=True)
        n_planned = len(tgts)
        tdir = lambda k: os.path.join(hd, "target" if n_planned == 1 else "target_e%d" % k)  # (by the plan: a skipped second video must not rename the first's)
        done = set(); man_eras = {}; timing = dict(gen={}, render={})
        L = lead(hi)
        ok, tg = wait_era(hi, L); timing["gen"][L] = round(tg, 1)
        if not ok: print("  era %d gen2 FAILED, see %s" % (L, os.path.join(hd, "_gen", "batch_0.log"))); return
        pl_ = os.path.join(hd, "_gen", "era_%d.pool.json" % L)
        if not os.path.exists(pl_) or not json.load(open(pl_)).get("views"):
            # (no room of the target type in this house -- open plans fold dining rooms and kitchens into the living room)
            print("  no %s in this house, or no camera view in it: home skipped" % h["room"], flush=True)
            for key in [x for x in jobs if x[0] == hi]:
                p_, lf_, _ = jobs.pop(key); p_.kill(); p_.wait(); lf_.close()
            started.update((hi, gi) for gi in range(len(groups(hi))))
            with open(os.path.join(out, "skipped.jsonl"), "a") as f: f.write(json.dumps(dict(home=h["name"], seed=h["seed"], room=h["room"], reason="no-target-room")) + "\n")
            shutil.rmtree(hd, ignore_errors=True)
            if hi + 1 < len(plans): start_job(hi + 1, 0)
            return
        shutil.copy(os.path.join(hd, "_gen", "era_%d.house.json" % L), os.path.join(hd, "shell.house.json"))
        fill(hi)
        refs_by_era = {}; shell_dropped = []; chosen = {}; videos = {}; queued = []; queued_at = {}; skipped_targets = []; late_eras = []; killed = set()
        import queue
        vq = queue.Queue()
        def _videos():
            # (the target videos one after another in the background: the photos load their scenes on the CPU while it keeps the GPU busy)
            while True:
                job = vq.get()
                if job is None: return
                k, glb_k, path_k, f_k = job
                try: videos[k] = render_video(a, cfg, tdir(k), glb_k, path_k, f_k, log)
                except Exception as ex: videos[k] = ex
        vth = threading.Thread(target=_videos, daemon=True); vth.start()  # (daemon: a home that fails leaves no thread waiting)
        # (the order the eras come out in: by position in their batch, the independent batches first)
        gs_ = groups(hi); order = [L] + [k for j in range(max(len(b) for _, b, _ in gs_)) for _, b, ind in gs_ if ind for k in b[j:j + 1] if k != L] + [k for _, b, ind in gs_ if not ind for k in b]
        for k in order:
            if k != L:
                # (an era that is not built while its photos and the finish still fit is left out: ~6 s of photos, ~3 s to finish)
                ok, tg = wait_era(hi, k, t_home + float(cfg["budget_s"]) - 9.0 if k not in tgts else None); timing["gen"][k] = round(tg, 1)
                if ok is None or (not ok and any(k in groups(hi)[g][1] for g in killed)):
                    print("  era %d left out: not built %.0f s into the home" % (k, time.time() - t_home), flush=True); late_eras.append(k)
                    gi = next(i for i, (_, ks, _) in enumerate(groups(hi)) if k in ks)
                    if (hi, gi) in jobs: jobs[(hi, gi)][0].kill(); killed.add(gi)
                    continue
                if not ok: print("  era %d gen2 FAILED, see the batch logs in %s" % (k, os.path.join(hd, "_gen"))); continue
            _, ed, glb = paths(hi, k); side = json.load(open(glb[:-4] + ".json"))
            hj = json.load(open(glb[:-4] + ".house.json")); sj = json.load(open(os.path.join(hd, "shell.house.json")))
            same_shell = hj["rooms"] == sj["rooms"] and sorted(map(json.dumps, hj["cutters"])) == sorted(map(json.dumps, sj["cutters"]))
            if not same_shell: print("  era %d: shell differs from era %d's (re-planned house?) -- era dropped" % (k, L)); shell_dropped.append(k); continue
            pj = json.load(open(os.path.join(hd, "_gen", "era_%d.pool.json" % k))); pool = pj["views"]
            n = int(cfg["target_photos"]) if k in tgts else int(cfg["photos_per_era"])
            if k in tgts:
                # (the walk first: the photos of every era are then picked partly for what the walks show)
                try: chosen[k] = choose_path(a, cfg, h, tdir(k), glb, pool, log, pj.get("video_paths"))
                except RuntimeError as ex:
                    # (no safe walk in this era: it stays a reference era)
                    print("  era %d: no video (%s)" % (k, ex), flush=True); skipped_targets.append(dict(era=k, reason=str(ex)))
                # (what the walks see, for the camera pools of the eras still being built: their views aim at it too)
                ap = np.concatenate([c[3] for c in chosen.values()]) if chosen else np.zeros((0, 3)); ap = ap[(ap[:, 2] > 0.2) & (ap[:, 2] < 2.4)]
                if len(ap):
                    ap = ap[np.random.default_rng(crc(h["seed"], k, "aim")).choice(len(ap), min(48, len(ap)), replace=False)]
                    tmpf = os.path.join(hd, "_gen", "aim_points.json.tmp"); json.dump(ap.round(3).tolist(), open(tmpf, "w")); os.replace(tmpf, os.path.join(hd, "_gen", "aim_points.json"))
                timing["render"]["path_%d" % k] = chosen[k][2]["choose_s"]
            vpts = [c[3] for c in chosen.values()]
            views, dropped, t_ph, kept = photos_for_era(a, cfg, eras[k], h, ed, glb, pool, n, log, vpts) if n else ([], [], 0.0, [])
            refs_by_era[k] = kept
            timing["render"]["era_%d" % k] = round(t_ph, 1)
            if k in tgts and k in chosen:
                # (a target after the first is rendered only while the home still fits its budget: the video's own time (measured on
                # the first, else ~0.3 s a frame), ~6 s for each era still to photograph, ~3 s to finish; else it stays a reference era)
                rate = [v["render_s"] / len(v["frames"]) for v in videos.values() if isinstance(v, dict)]
                vid = int(cfg["target"]["frames"]) * (rate[0] if rate else 0.2)
                now = time.time() - t_home
                # (the queue ahead: what is left of the videos already queued, from when each was queued)
                ahead = sum(max(0.0, qt + vid - now) for kk, qt in queued_at.items() if not isinstance(videos.get(kk), dict))
                est = now + ahead + vid + 6.0 * (len(order) - order.index(k) - 1) + 3.0
                if queued and est > float(cfg["budget_s"]):
                    print("  era %d: second video skipped (would end near %.0f s of %.0f)" % (k, est, cfg["budget_s"]), flush=True)
                    skipped_targets.append(dict(era=k, estimate_s=round(est, 1)))
                else:
                    vq.put((k, glb, chosen[k][0], chosen[k][1])); queued.append(k); queued_at[k] = time.time() - t_home
            var = side.get("variation", {})
            ppl = sorted({nd["name"].split("#")[0] for nd in side["nodes"] if nd["name"].startswith("person")})
            fin = {nd["name"]: nd["spec"]["cls"] for nd in side["nodes"] if nd.get("class") == "room" and nd.get("spec") and nd["name"].startswith(room_slug(h["room"]))}
            left = None
            if eras[k]["state"]["furniture"].get("empty"):
                # (an empty era's leftovers in the photographed room: movable pieces the emptying missed)
                left = sorted({o["class"] for o in side.get("objects", []) if (o.get("room") or "").startswith(room_slug(h["room"])) and not any(c in (o.get("class") or "") for c in KEEP_BUILTIN)})
            rec = dict(era=k, dir="era_%d" % k, changed=eras[k]["changed"], state=eras[k]["state"], photos=views, dropped=dropped, people=ppl,
                       finishes=fin, lighting={x: var.get("lighting", {}).get(x) for x in ("time_of_day_h", "sun_dir", "sun_strength", "sky_strength", "is_night", "lamps_on", "lamps_total")},
                       variation_furniture=var.get("furniture"), empty_leftovers=left)
            man_eras[k] = rec; done.add(k)
            print("  era %d: %d photos (%s) gen %.1fs render %.1fs%s%s" % (k, len(views), ", ".join(eras[k]["changed"]) or "base", timing["gen"].get(k, 0), t_ph,
                  "  + video (background, %s path)" % chosen[k][2]["family"] if k in tgts else "", "  dropped %s" % [d["reason"].split(" ")[0] for d in dropped] if dropped else ""), flush=True)
            if not cfg["keep_glb"] and k not in tgts:
                for x in (glb, glb[:-4] + ".json"): os.path.exists(x) and os.remove(x)
        vq.put(None); vth.join()
        t_fin = time.time()
        q = cfg["qc"]; fails = []; targets = []
        planned = list(tgts); tgts = [k for k in tgts if k in queued]  # (skipped targets are reference eras now, with the target's photo count)
        for k in tgts:
            v = videos.get(k)
            if not isinstance(v, dict): fails.append("target era %d: no video (%s)" % (k, v)); continue
            timing["render"]["video_%d" % k] = round(v["render_s"], 1)
            labels = video_labels(tdir(k), v, refs_by_era, k); v.pop("_small", None)
            for kk, ov in labels.pop("per_photo").items():
                for ph, o in zip(man_eras[kk]["photos"] if kk in man_eras else [], ov): ph.setdefault("video_overlap", {})["era_%d" % k] = o
            pinfo = chosen[k][2]
            if pinfo["close_share"] > q["max_close"]: fails.append("era %d video close to walls in %.0f %% of its frames" % (k, 100 * pinfo["close_share"]))
            if labels["share"]["seen"] < q["min_seen"]: fails.append("era %d: references show %.0f %% of its video" % (k, 100 * labels["share"]["seen"]))
            targets.append(dict(era=k, dir=os.path.relpath(tdir(k), hd), masks=dict(labels, against="photos of the other eras"), video_path=pinfo, **v))
        for k, rec in man_eras.items():
            json.dump(dict(rec, convention="OpenCV cameras: x right, y down, z forward; c2w / w2c world z up, metres; depth = z-depth, 0 = none; "
                           "K in pixels of this photo, K_normalized by its width / height; video_overlap = share of each target video's surface this photo shows"),
                      open(os.path.join(hd, rec["dir"], "cameras.json"), "w"), indent=1)
        # quality gate
        for k, rec in man_eras.items():
            need = q["min_photos"] if k not in planned else min(q["min_photos"], int(cfg["target_photos"]))
            if k not in tgts and len(rec["photos"]) < need: fails.append("era %d has %d photos" % (k, len(rec["photos"])))
            if rec["empty_leftovers"]: fails.append("empty era %d keeps %s" % (k, rec["empty_leftovers"]))
        if len([k for k in man_eras if k not in tgts]) < 1: fails.append("no reference era")
        if shell_dropped: fails.append("eras %s dropped (shell differs)" % shell_dropped)
        if not targets: print("  no target video: home dropped"); fails.append("no target video")
        qc = dict(passed=not fails, fails=fails, dropped_photos=sum(len(r["dropped"]) for r in man_eras.values()), skipped_targets=skipped_targets, late_eras=late_eras)
        for v in targets:
            enc = v.pop("_enc", None)
            if enc: enc.wait()
        man = dict(home=h["name"], seed=h["seed"], room=h["room"], levels=cfg["levels"], size=[W, H], qc=qc,
                   eras=[man_eras[k] for k in sorted(man_eras)], target=targets[0] if targets else None, targets=targets,
                   timing=dict(timing, total_s=round(time.time() - t_home, 1)))
        json.dump(man, open(os.path.join(hd, "manifest.json"), "w"), indent=1)
        if targets: contact_sheet(hd, man, os.path.join(hd, "sheet.png"))
        if not cfg["keep_glb"]: shutil.rmtree(os.path.join(hd, "_gen"), ignore_errors=True)
        sh = [t["masks"]["share"] for t in targets]
        labels = dict(share={c: round(sum(x[c] for x in sh) / max(1, len(sh)), 3) for c in ("seen", "changed", "hidden", "never")})
        row = dict(home=h["name"], seed=h["seed"], room=h["room"], eras=len(man["eras"]), target_eras=[t["era"] for t in targets], photos=sum(len(e["photos"]) for e in man["eras"]),
                   frames=sum(len(t["frames"]) for t in targets), share=labels["share"], paths=[t["video_path"]["family"] for t in targets],
                   cells=[t["video_path"]["cells"] for t in targets], total_s=man["timing"]["total_s"], qc=qc["passed"], fails=fails)
        if not qc["passed"]:
            # (a home that fails the gate is kept apart, with its reasons, out of the training index)
            rj = os.path.join(out, "_rejected"); os.makedirs(rj, exist_ok=True); shutil.rmtree(os.path.join(rj, h["name"]), ignore_errors=True)
            shutil.move(hd, os.path.join(rj, h["name"]))
        with open(os.path.join(out, "index.jsonl" if qc["passed"] else "rejected.jsonl"), "a") as f: f.write(json.dumps(row) + "\n")
        print("%s %s in %.1fs (gen %s, render %s, finish %.1fs) seen %.0f %% changed %.0f %% never %.0f %%%s" % ("done" if qc["passed"] else "REJECTED", h["name"], time.time() - t_home,
              timing["gen"], timing["render"], time.time() - t_fin, 100 * labels["share"]["seen"], 100 * labels["share"]["changed"], 100 * labels["share"]["never"],
              "" if qc["passed"] else ": " + "; ".join(fails)), flush=True)

    for hi, (h, eras, tgts) in enumerate(plans):
        try: build_home(hi, h, eras, tgts)
        except Exception as ex:
            # (one home's error does not end the run: it is listed with the rejected ones)
            import traceback; traceback.print_exc()
            with open(os.path.join(out, "rejected.jsonl"), "a") as f: f.write(json.dumps(dict(home=h["name"], seed=h["seed"], room=h["room"], qc=False, fails=["error: %s" % ex])) + "\n")
            for key in [x for x in jobs if x[0] == hi]: jobs[key][0].kill()


if __name__ == "__main__":
    main()

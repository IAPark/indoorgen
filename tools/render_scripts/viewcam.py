"""viewcam.py: eval camera picker shared by render_ref.py and render_rust.py.

Uses only layout-level geometry so that two scenes with the same layout get the same camera regardless of mesh detail:
the floor outline (convex hull of the floor vertices with tiny features such as door notches dropped), its area
centroid, and world AABBs of the furniture (room shell, placeholders, windows and doors excluded).
cornerA / cornerB = best / second-best outline corner (camera 0.45 m inside, 1.5 m high, looking at the centre)."""
import math


def _hull(pts):
    pts = sorted(set(pts))
    if len(pts) < 3:
        return pts
    def cr(o, a, b): return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lo, up = [], []
    for p in pts:
        while len(lo) >= 2 and cr(lo[-2], lo[-1], p) <= 1e-9: lo.pop()
        lo.append(p)
    for p in reversed(pts):
        while len(up) >= 2 and cr(up[-2], up[-1], p) <= 1e-9: up.pop()
        up.append(p)
    return lo[:-1] + up[:-1]


def _simplify(h, min_dev=0.25):
    # drop outline vertices that stick out less than min_dev from the line through their neighbours (door notches etc.)
    h = list(h)
    while len(h) > 3:
        def dev(i):
            a, b, c = h[i - 1], h[i], h[(i + 1) % len(h)]
            L = math.hypot(c[0] - a[0], c[1] - a[1]) or 1e-9
            return abs((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / L
        i = min(range(len(h)), key=dev)
        if dev(i) >= min_dev: break
        h.pop(i)
    return h


def _ray_box(o, d, b, tmax):
    t0, t1 = 0.0, tmax
    for k, (lo, hi) in enumerate(((b[0], b[1]), (b[2], b[3]))):
        if abs(d[k]) < 1e-12:
            if not (lo <= o[k] <= hi): return False
        else:
            a, c = (lo - o[k]) / d[k], (hi - o[k]) / d[k]
            if a > c: a, c = c, a
            t0, t1 = max(t0, a), min(t1, c)
            if t0 > t1: return False
    return True


def _ray_seg(o, d, p, q, tmax):
    ex, ey = q[0] - p[0], q[1] - p[1]
    den = d[0] * ey - d[1] * ex
    if abs(den) < 1e-12: return False
    t = ((p[0] - o[0]) * ey - (p[1] - o[1]) * ex) / den
    s = ((p[0] - o[0]) * d[1] - (p[1] - o[1]) * d[0]) / den
    return 1e-6 < t < tmax and 0 <= s <= 1


def view_camera(sc, room, view):
    import bpy, mathutils, bmesh
    fl = [o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith(room) and ".floor" in o.name]
    pts = []; area = 0.0; ax = ay = 0.0
    for o in fl:
        b = bmesh.new(); b.from_mesh(o.data); b.transform(o.matrix_world)
        for f in b.faces:
            a = f.calc_area(); c = f.calc_center_median(); area += a; ax += c.x * a; ay += c.y * a
        pts += [(round(v.co.x, 4), round(v.co.y, 4)) for v in b.verts]
        b.free()
    cen = (ax / area, ay / area)
    outline = _simplify(_hull(pts))
    print("VIEWOUT", [(round(a, 2), round(b, 2)) for a, b in outline], "floors", [o.name for o in fl])
    skip = (".floor", ".wall", ".ceiling", "skirting", "exterior", "entrance", "placeholder", "vc", "portal", "window", "door")
    groups = {}
    def key(o):
        r = o
        while r.parent is not None and r.parent.type == "MESH": r = r.parent
        return r.name.split("#")[0]  # Rust exports asset parts as "<id>#<k>"; python joins an asset or parents its parts
    for o in bpy.data.objects:
        n = o.name.lower()
        if o.type != "MESH" or o.hide_render or any(k in n for k in skip) or o.name.endswith(".meshed"): continue
        if any(c.name.startswith("placeholders") for c in o.users_collection): continue
        bb = [o.matrix_world @ mathutils.Vector(c) for c in o.bound_box]
        z0 = min(b.z for b in bb); z1 = max(b.z for b in bb)
        if z0 >= 1.9 or z1 <= 0.3: continue
        g = groups.setdefault(key(o), [9e9, -9e9, 9e9, -9e9])
        g[0] = min(g[0], min(b.x for b in bb)); g[1] = max(g[1], max(b.x for b in bb))
        g[2] = min(g[2], min(b.y for b in bb)); g[3] = max(g[3], max(b.y for b in bb))
    boxes = [tuple(g) for g in groups.values()]
    def blocked(q):
        return any(x0 - 0.8 < q[0] < x1 + 0.8 and y0 - 0.8 < q[1] < y1 + 0.8 for x0, x1, y0, y1 in boxes)
    def clearance(q):
        best = 9.0
        for x0, x1, y0, y1 in boxes:
            best = min(best, math.hypot(max(x0 - q[0], 0, q[0] - x1), max(y0 - q[1], 0, q[1] - y1)))
        return best
    def openness(q):
        fx, fy = cen[0] - q[0], cen[1] - q[1]; L = math.hypot(fx, fy) or 1.0; fx, fy = fx / L, fy / L
        n = 0
        for k in range(-4, 5):
            a = math.radians(10 * k); d = (fx * math.cos(a) - fy * math.sin(a), fx * math.sin(a) + fy * math.cos(a))
            hit = any(_ray_box(q, d, b, 2.0) for b in boxes if not (b[0] <= q[0] <= b[1] and b[2] <= q[1] <= b[3])) or any(_ray_seg(q, d, outline[i - 1], outline[i], 2.0) for i in range(len(outline)))
            n += 0 if hit else 1
        return n
    cands = sorted(outline, key=lambda p: -math.hypot(p[0] - cen[0], p[1] - cen[1]))[:6]
    scored = []
    for p in cands:
        dx, dy = cen[0] - p[0], cen[1] - p[1]; L = math.hypot(dx, dy) or 1.0
        q = (p[0] + dx / L * 0.45, p[1] + dy / L * 0.45)
        scored.append((not blocked(q), openness(q), clearance(q), p, q))
    print("VIEWCAND", [(a, b, round(c, 2), round(p[0], 2), round(p[1], 2)) for a, b, c, p, _ in scored])
    order = sorted(range(len(scored)), key=lambda i: (-(scored[i][0] and scored[i][1] >= 8), -(scored[i][1] >= 7), -round(min(scored[i][2], 0.6), 1), -scored[i][1], i))
    q = scored[order[1 if view == "cornerB" and len(order) > 1 else 0]][4]
    loc = mathutils.Vector((q[0], q[1], 1.5)); tgt = mathutils.Vector((cen[0], cen[1], 0.9))
    cam = bpy.data.cameras.new("vc"); cam.lens = 18; cam.sensor_width = 36
    co = bpy.data.objects.new("vc", cam); sc.collection.objects.link(co)
    co.location = loc; print("VIEWCAM", view, tuple(round(x, 2) for x in loc))
    co.rotation_euler = (tgt - loc).to_track_quat("-Z", "Y").to_euler()
    sc.camera = co

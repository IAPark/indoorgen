"""viewcam.py: eval camera picker shared by render_ref.py and render_rust.py.

Uses only layout-level geometry so that two scenes with the same layout get the same camera regardless of mesh detail:
the floor outline (convex hull of the floor vertices with tiny features such as door notches dropped), its area
centroid, and world AABBs of the furniture (room shell, placeholders, windows and doors excluded).
cornerA / cornerB = best / second-best outline corner (camera 0.45 m inside, 1.5 m high, looking at the centre)."""
import math
import re


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


def _ray_t(o, d, b, tmax):
    """entry distance of the ray o + t d (t in [0, tmax]) into the box b = (x0, x1, y0, y1), or None"""
    t0, t1 = 0.0, tmax
    for k, (lo, hi) in enumerate(((b[0], b[1]), (b[2], b[3]))):
        if abs(d[k]) < 1e-12:
            if not (lo <= o[k] <= hi): return None
        else:
            a, c = (lo - o[k]) / d[k], (hi - o[k]) / d[k]
            if a > c: a, c = c, a
            t0, t1 = max(t0, a), min(t1, c)
            if t0 > t1: return None
    return t0


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
    pts = []; area = 0.0; ax = ay = 0.0; tris = []
    for o in fl:
        b = bmesh.new(); b.from_mesh(o.data); b.transform(o.matrix_world)
        for f in b.faces:
            a = f.calc_area(); c = f.calc_center_median(); area += a; ax += c.x * a; ay += c.y * a
            vs = [(v.co.x, v.co.y) for v in f.verts]
            tris += [(vs[0], vs[i], vs[i + 1]) for i in range(1, len(vs) - 1)]
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
    # open door leaves (skipped above with the frames): a corner behind one framed the leaf and little else; they count for the
    # openness rays and the clearance, not for `blocked` (a door near a corner is common and the spot stays usable)
    import re
    leaves = []
    for o in bpy.data.objects:
        if o.type != "MESH" or not re.match(r"door\d+(#|$)", o.name.lower()): continue
        bb = [o.matrix_world @ mathutils.Vector(c) for c in o.bound_box]
        leaves.append((min(b.x for b in bb), max(b.x for b in bb), min(b.y for b in bb), max(b.y for b in bb)))
    def blocked(q):
        return any(x0 - 0.8 < q[0] < x1 + 0.8 and y0 - 0.8 < q[1] < y1 + 0.8 for x0, x1, y0, y1 in boxes)
    def clearance(q):
        best = 9.0
        for x0, x1, y0, y1 in boxes + leaves:
            best = min(best, math.hypot(max(x0 - q[0], 0, q[0] - x1), max(y0 - q[1], 0, q[1] - y1)))
        return best
    def openness(q):
        fx, fy = cen[0] - q[0], cen[1] - q[1]; L = math.hypot(fx, fy) or 1.0; fx, fy = fx / L, fy / L
        n = 0
        for k in range(-4, 5):
            a = math.radians(10 * k); d = (fx * math.cos(a) - fy * math.sin(a), fx * math.sin(a) + fy * math.cos(a))
            hit = any(_ray_box(q, d, b, 2.0) for b in boxes + leaves if not (b[0] <= q[0] <= b[1] and b[2] <= q[1] <= b[3])) or any(_ray_seg(q, d, outline[i - 1], outline[i], 2.0) for i in range(len(outline)))
            n += 0 if hit else 1
        return n
    def content(q):
        # rays (+-40 deg about the centre direction) that meet furniture 1.2-5 m out: something to look at, not empty floor
        fx, fy = cen[0] - q[0], cen[1] - q[1]; L = math.hypot(fx, fy) or 1.0
        n = 0
        for k in range(-8, 9):
            a = math.radians(5 * k); d = ((fx * math.cos(a) - fy * math.sin(a)) / L, (fx * math.sin(a) + fy * math.cos(a)) / L)
            far = any(_ray_box(q, d, b, 5.0) for b in boxes if not (b[0] <= q[0] <= b[1] and b[2] <= q[1] <= b[3]))
            near = any(_ray_box(q, d, b, 1.2) for b in boxes if not (b[0] <= q[0] <= b[1] and b[2] <= q[1] <= b[3]))
            n += far and not near
        return n
    def leaf_near(q):
        # a door leaf within 1.5 m inside the view cone (+-50 deg about the centre direction): it fills a side of the frame
        fx, fy = cen[0] - q[0], cen[1] - q[1]; L = math.hypot(fx, fy) or 1.0
        for k in range(-10, 11):
            a = math.radians(5 * k); d = ((fx * math.cos(a) - fy * math.sin(a)) / L, (fx * math.sin(a) + fy * math.cos(a)) / L)
            if any(_ray_box(q, d, b, 1.5) for b in leaves): return True
        return False
    def on_floor(q):
        def inside(t, p):
            (ax_, ay_), (bx_, by_), (cx_, cy_) = t
            d1 = (p[0] - bx_) * (ay_ - by_) - (ax_ - bx_) * (p[1] - by_); d2 = (p[0] - cx_) * (by_ - cy_) - (bx_ - cx_) * (p[1] - cy_); d3 = (p[0] - ax_) * (cy_ - ay_) - (cx_ - ax_) * (p[1] - ay_)
            return not ((d1 < 0 or d2 < 0 or d3 < 0) and (d1 > 0 or d2 > 0 or d3 > 0))
        # the spot and 0.3 m around it on the room's floor (the outline is the floor's hull: an L room's hull edge midpoint stood in a wall)
        return all(any(inside(t, (q[0] + dx, q[1] + dy)) for t in tris) for dx, dy in ((0, 0), (0.3, 0), (-0.3, 0), (0, 0.3), (0, -0.3)))
    ceil = [o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith(room) and ".ceiling" in o.name]
    cz = max(((o.matrix_world @ mathutils.Vector(c)).z for o in ceil for c in o.bound_box), default=None)
    dg = bpy.context.evaluated_depsgraph_get()
    def headroom(q):
        # nothing over the camera short of the ceiling (a window alcove's header: the spot was on the floor, the view a black slab)
        if cz is None: return True
        hit, loc, *_ = sc.ray_cast(dg, mathutils.Vector((q[0], q[1], 1.5)), mathutils.Vector((0, 0, 1)))
        return not hit or loc.z > cz - 0.1
    cands = sorted(outline, key=lambda p: -math.hypot(p[0] - cen[0], p[1] - cen[1]))[:6]
    # (and the walls' midpoints, after the corners: a small room's corners can all stand against furniture -- a wardrobe's side
    # filled half the frame)
    cands += [((outline[i - 1][0] + outline[i][0]) / 2, (outline[i - 1][1] + outline[i][1]) / 2) for i in range(len(outline))
              if math.hypot(outline[i][0] - outline[i - 1][0], outline[i][1] - outline[i - 1][1]) > 1.5]
    scored = []
    for p in cands:
        dx, dy = cen[0] - p[0], cen[1] - p[1]; L = math.hypot(dx, dy) or 1.0
        # 0.45 m in; deeper when that spot is not usable (a kitchen's corners all have a wall cabinet at camera height)
        for inset in (0.45, 1.0, 1.4):
            q = (p[0] + dx / L * inset, p[1] + dy / L * inset)
            if inset >= L - 0.3 or not on_floor(q) or not headroom(q): continue
            scored.append((not blocked(q), openness(q), clearance(q), p, q, not leaf_near(q), content(q)))
    # score each spot by what the camera would see: a 24 x 14 grid of rays through the frame (18 mm lens, 16:9) against the scene.
    # Footprint boxes misjudged it every way (a wardrobe's side filled the frame, a duvet the foreground, a blank wall the rest):
    # + each different piece of furniture seen, + the share of the frame on furniture 1-5 m out, - the share within 1.4 m of the lens,
    # - the share on a door leaf, - the share on a wall within 1.8 m. The heading is searched +-30 deg about the room's centre; the pitch is fixed (3 m out, 0.75 m up).
    fixed = ("wall", "floor", "ceiling", "skirting", "exterior", "entrance", "window", "portal", "door", "frame", "light")
    leaf_re = re.compile(r"door\d+(#|$)")
    def view_score(q, yaw):
        fx, fy = cen[0] - q[0], cen[1] - q[1]; L = math.hypot(fx, fy) or 1.0
        c_, s_ = math.cos(yaw), math.sin(yaw)
        ux, uy = (fx * c_ - fy * s_) / L, (fx * s_ + fy * c_) / L
        o = mathutils.Vector((q[0], q[1], 1.5))
        fwd = (mathutils.Vector((q[0] + ux * 3.0, q[1] + uy * 3.0, 0.75)) - o).normalized()
        right = fwd.cross(mathutils.Vector((0, 0, 1))).normalized(); up = right.cross(fwd)
        seen, furn, near, leaf, wall, n = set(), 0, 0, 0, 0, 0
        for i in range(24):
            for j in range(14):
                d = (fwd + right * ((i + 0.5) / 24 * 2 - 1) + up * ((j + 0.5) / 14 * 2 - 1) * 0.5625).normalized()
                hit, loc, nrm, _, ob, _ = sc.ray_cast(dg, o, d, distance=8.0)
                n += 1
                if not hit: continue
                dist = (loc - o).length; name = ob.name.lower()
                if leaf_re.match(name): leaf += 1; continue
                if dist < 1.4: near += 1
                if any(k in name for k in fixed):
                    # (a bare wall close by: a third of a bedroom view was the empty wall beside the camera)
                    if dist < 1.8 and abs(nrm.z) < 0.5: wall += 1
                    continue
                if dist <= 5.0: furn += 1; seen.add(key(ob))
        return 2.0 * len(seen) + 30.0 * furn / n - 50.0 * near / n - 60.0 * leaf / n - 25.0 * wall / n
    views = []
    for (_, _, cl, p, q, _, _) in scored:
        sv, yaw = max(((view_score(q, math.radians(5 * k)), math.radians(5 * k)) for k in range(-8, 9)), key=lambda t: (t[0], -abs(t[1])))
        views.append((sv, yaw, p, q))
    print("VIEWCAND", [(round(sv, 1), round(math.degrees(yaw)), round(q[0], 2), round(q[1], 2)) for sv, yaw, p, q in views])
    order = sorted(range(len(views)), key=lambda i: -views[i][0])
    pick = order[0]
    if view == "cornerB":
        # the best view from another spot, 1.5 m or more from cornerA's
        qa = views[order[0]][3]
        pick = next((i for i in order[1:] if math.hypot(views[i][3][0] - qa[0], views[i][3][1] - qa[1]) >= 1.5), order[min(1, len(order) - 1)])
    sv, yaw, _, q = views[pick]
    fx, fy = cen[0] - q[0], cen[1] - q[1]; L = math.hypot(fx, fy) or 1.0
    ux, uy = (fx * math.cos(yaw) - fy * math.sin(yaw)) / L, (fx * math.sin(yaw) + fy * math.cos(yaw)) / L
    print("VIEWYAW", round(math.degrees(yaw)), round(sv, 1))
    loc = mathutils.Vector((q[0], q[1], 1.5)); tgt = mathutils.Vector((q[0] + ux * 3.0, q[1] + uy * 3.0, 0.75))
    cam = bpy.data.cameras.new("vc"); cam.lens = 18; cam.sensor_width = 36
    co = bpy.data.objects.new("vc", cam); sc.collection.objects.link(co)
    co.location = loc; print("VIEWCAM", view, tuple(round(x, 2) for x in loc))
    co.rotation_euler = (tgt - loc).to_track_quat("-Z", "Y").to_euler()
    sc.camera = co

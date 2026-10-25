"""Render a reference blend: render_ref.py scene.blend out.png [--top] [--clay] [--samples N] [--cam cam.json]"""
import sys, argparse, json, time, bpy, mathutils
ap = argparse.ArgumentParser(); ap.add_argument("blend"); ap.add_argument("out"); ap.add_argument("--top", action="store_true"); ap.add_argument("--clay", action="store_true")
ap.add_argument("--samples", type=int, default=16); ap.add_argument("--cam"); ap.add_argument("--room"); ap.add_argument("--view"); ap.add_argument("--nopoint", action="store_true")
args = ap.parse_args()
bpy.ops.wm.open_mainfile(filepath=args.blend)
if args.nopoint:
    for o in bpy.data.objects:
        if o.type=="LIGHT" and o.data.type=="POINT": o.hide_render=True
sc = bpy.context.scene
if args.clay:
    clay = bpy.data.materials.new("clay"); clay.use_nodes = True
    clay.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.6, 0.6, 0.6, 1)
    clay.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.9
for o in list(bpy.data.objects):
    if o.type == "MESH":
        if "placeholders" in [c.name for c in o.users_collection] or o.name.endswith(".exterior"): o.hide_render = True; continue
        if args.clay:
            o.data.materials.clear(); o.data.materials.append(clay)
def view_camera(sc, room, view):
    """cornerA / cornerB views inside `room`: farthest floor corners from the centroid; skips corners whose camera spot is blocked"""
    import bpy, mathutils, math
    fl = [o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith(room) and ".floor" in o.name]
    # layout-only inputs (independent of how the floor is triangulated/subdivided): area-weighted centroid, and the
    # outline corners (boundary vertices where the outline turns)
    import bmesh as _bm
    pts = []; area = 0.0; acc = mathutils.Vector((0, 0, 0))
    for o in fl:
        b = _bm.new(); b.from_mesh(o.data); b.transform(o.matrix_world)
        for f in b.faces:
            a = f.calc_area(); area += a; acc += f.calc_center_median() * a
        for v in b.verts:
            be = [e for e in v.link_edges if e.is_boundary]
            if len(be) != 2: continue
            p0 = be[0].other_vert(v).co - v.co; p1 = be[1].other_vert(v).co - v.co
            if p0.length < 1e-6 or p1.length < 1e-6: continue
            if abs(p0.x * p1.y - p0.y * p1.x) / (p0.length * p1.length) > 0.05:
                pts.append(mathutils.Vector((v.co.x, v.co.y, 0)))
        b.free()
    cen = mathutils.Vector((acc.x / area, acc.y / area, 0))
    # blockers: every visible object except the room shell / window frames+glass (curtains and doors DO block the camera)
    skip = (".floor", ".wall", ".ceiling", "skirting", "exterior", "entrance", "placeholder", "vc", "portal")
    def is_frame(n): return "window" in n or "door" in n  # doors: python door objects carry a large local AABB, Rust doors are split into parts; python joins frame+shutter+curtain into one window object; treat Rust parts the same
    boxes = []
    for o in bpy.data.objects:
        if o.type != "MESH" or o.hide_render or any(k in o.name.lower() for k in skip) or is_frame(o.name.lower()): continue
        if any(c.name.startswith("placeholders") for c in o.users_collection) or o.name.endswith(".meshed"): continue
        bb = [o.matrix_world @ mathutils.Vector(c) for c in o.bound_box]
        boxes.append((min(b.x for b in bb), max(b.x for b in bb), min(b.y for b in bb), max(b.y for b in bb), min(b.z for b in bb), max(b.z for b in bb)))
    def blocked(loc):
        for x0, x1, y0, y1, z0, z1 in boxes:
            if x0 - 0.8 < loc.x < x1 + 0.8 and y0 - 0.8 < loc.y < y1 + 0.8 and z0 < 1.9 and z1 > 0.3: return True
        return False
    uniq = []
    for p in sorted(pts, key=lambda p: -(p - cen).length):
        if all((p - q).length > 0.8 for q in uniq): uniq.append(p)
    cands = uniq[:6]
    def clearance(loc):
        best = 9.0
        for x0, x1, y0, y1, z0, z1 in boxes:
            if z0 >= 1.9 or z1 <= 0.3: continue
            dx = max(x0 - loc.x, 0, loc.x - x1); dy = max(y0 - loc.y, 0, loc.y - y1)
            best = min(best, math.hypot(dx, dy))
        return best
    far = None
    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get()
    def openness(loc):
        # rays across the horizontal field of view toward the room centre: how many travel > 1.2 m before hitting anything
        o3 = mathutils.Vector((loc.x, loc.y, 1.5)); t3 = mathutils.Vector((cen.x, cen.y, 0.9))
        fwd = (t3 - o3).normalized(); n = 0
        for k in range(-4, 5):
            rot = mathutils.Matrix.Rotation(math.radians(10 * k), 3, "Z")
            hit, *_ = sc.ray_cast(dg, o3, (rot @ fwd).normalized(), distance=2.0)
            n += 0 if hit else 1
        return n
    scored = []
    for p in cands:
        d = (cen - p); d.normalize(); q = p + d * 0.45
        scored.append((not blocked(q), openness(q), clearance(q), p))
    print("VIEWCAND", [(a, b, round(c, 2), round(p.x, 2), round(p.y, 2)) for a, b, c, p in scored]); print("VIEWBOX", sorted((round(b[0],1),round(b[1],1),round(b[2],1),round(b[3],1)) for b in boxes))
    # rank the corner spots: unblocked and open first, then openness, clearance; cornerA = best, cornerB = second best
    order = sorted(range(len(scored)), key=lambda i: (-(scored[i][0] and scored[i][1] >= 8), -(scored[i][1] >= 7), -min(scored[i][2], 0.6), -scored[i][1], i))
    far = scored[order[1 if view == "cornerB" and len(order) > 1 else 0]][3]
    d = (cen - far); d.normalize()
    loc = far + d * 0.45; loc.z = 1.5
    tgt = mathutils.Vector((cen.x, cen.y, 0.9))
    cam = bpy.data.cameras.new("vc"); cam.lens = 18; cam.sensor_width = 36
    co = bpy.data.objects.new("vc", cam); sc.collection.objects.link(co)
    co.location = loc; print("VIEWCAM", view, tuple(round(x, 2) for x in loc))
    co.rotation_euler = (tgt - loc).to_track_quat("-Z", "Y").to_euler()
    sc.camera = co

if args.top:
    for o in list(bpy.data.objects):
        if o.type == "LIGHT" or o.type == "CAMERA": bpy.data.objects.remove(o)
        elif "ceiling" in o.name.lower(): o.hide_render = True
    meshes = [o for o in bpy.data.objects if o.type == "MESH" and not o.hide_render]
    pts = [o.matrix_world @ mathutils.Vector(c) for o in meshes if "floor" in o.name.lower() for c in o.bound_box]
    x0 = min(p.x for p in pts); x1 = max(p.x for p in pts); y0 = min(p.y for p in pts); y1 = max(p.y for p in pts)
    cam = bpy.data.cameras.new("c"); cam.type = "ORTHO"; cam.ortho_scale = max(x1-x0, y1-y0) * 1.1
    co = bpy.data.objects.new("c", cam); sc.collection.objects.link(co); co.location = ((x0+x1)/2, (y0+y1)/2, 30); sc.camera = co
    sun = bpy.data.lights.new("s", "SUN"); sun.energy = 4
    so = bpy.data.objects.new("s", sun); sc.collection.objects.link(so); so.rotation_euler = (0.3, 0.2, 0)
    w = bpy.data.worlds.new("w2"); w.use_nodes = True; w.node_tree.nodes["Background"].inputs[1].default_value = 1.0; sc.world = w
    sc.render.resolution_x = sc.render.resolution_y = 640

elif args.view in ("cornerA", "cornerB"):
    import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import viewcam; viewcam.view_camera(sc, args.room, args.view)
    sc.render.resolution_x, sc.render.resolution_y = 960, 540
else:
    if args.cam:
        _c = json.load(open(args.cam)); _cd = bpy.data.cameras.new("cjson"); _co = bpy.data.objects.new("cjson", _cd); sc.collection.objects.link(_co)
        _co.matrix_world = mathutils.Matrix(_c["matrix"]); _cd.lens = _c.get("lens", 15); _cd.sensor_width = _c.get("sensor_width", 36); sc.camera = _co
    else:
        cams = [o for o in bpy.data.objects if o.type == "CAMERA"]; sc.camera = cams[0]
    sc.render.resolution_x, sc.render.resolution_y = 960, 540
sc.render.engine = "CYCLES"; sc.cycles.samples = args.samples; sc.cycles.device = "CPU"; sc.cycles.use_denoising = True
import os as _os
sc.render.resolution_percentage = int(_os.environ.get("RES_PCT", "100"))
sc.cycles.use_adaptive_sampling = True
sc.render.filepath = args.out
if _os.environ.get("NORENDER"): sys.exit(0)
t = time.time(); bpy.ops.render.render(write_still=True); print("render time", time.time() - t)

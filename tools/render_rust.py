"""Render a Rust-generated scene (glb + sidecar json) with Infinigen's own materials/lighting.
usage: render_rust.py scene.glb out.png --cam camera.json [--samples N] [--top]"""
import os, sys, json, math, time, argparse
import bpy, mathutils
ap = argparse.ArgumentParser()
ap.add_argument("glb"); ap.add_argument("out")
ap.add_argument("--cam"); ap.add_argument("--samples", type=int, default=16); ap.add_argument("--top", action="store_true"); ap.add_argument("--showcase", action="store_true")
ap.add_argument("--seed", type=int, default=0); ap.add_argument("--clay", action="store_true"); ap.add_argument("--room"); ap.add_argument("--light-scale", type=float, default=1.0); ap.add_argument("--view"); ap.add_argument("--norender", action="store_true"); ap.add_argument("--no-cloth", action="store_true", help="keep the Rust cloth approximation instead of re-simulating in Blender")
args = ap.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:])
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=args.glb, merge_vertices=True)
import math
for _o in bpy.data.objects:
    if _o.type == 'MESH':
        bpy.ops.object.select_all(action='DESELECT'); _o.select_set(True); bpy.context.view_layer.objects.active = _o
        bpy.ops.object.shade_smooth_by_angle(angle=math.radians(35))

sidecar = json.load(open(args.glb.replace(".glb", ".json")))
if sidecar.get("cloth") and not args.no_cloth:
    # Rust exported the exact pre-sim cloth + python's cloth_sim settings: redo the sims with Blender's solver (tools/cloth_jobs.py)
    import numpy as _np
    for _t in (os.environ.get("INFINIGEN_RS_TOOLS"), os.path.expanduser("~/work/infinigen-rs/tools"), os.path.expanduser("~/work/wt/seat/tools")):
        if _t and os.path.exists(os.path.join(_t, "cloth_jobs.py")):
            sys.path.insert(0, _t); break
    import cloth_jobs
    def _world_mesh(name):
        o = bpy.data.objects[name]
        dg = bpy.context.evaluated_depsgraph_get(); e = o.evaluated_get(dg); me = e.to_mesh()
        M = _np.array(o.matrix_world)
        v = _np.array([tuple(x.co) for x in me.vertices]).reshape(-1, 3)
        v = v @ M[:3, :3].T + M[:3, 3]
        f = [list(p.vertices) for p in me.polygons]
        e.to_mesh_clear()
        return v, f
    def _set_mesh(name, v, f):
        o = bpy.data.objects[name]
        Mi = _np.array(o.matrix_world.inverted())
        lv = _np.asarray(v) @ Mi[:3, :3].T + Mi[:3, 3]
        o.data.clear_geometry()
        o.data.from_pydata([tuple(x) for x in lv], [], [list(map(int, p)) for p in f])
        o.data.update()
        bpy.ops.object.select_all(action='DESELECT'); o.select_set(True); bpy.context.view_layer.objects.active = o
        bpy.ops.object.shade_smooth_by_angle(angle=math.radians(35))
    try:
        cloth_jobs.run_jobs(sidecar["cloth"], _world_mesh, _set_mesh)
    except Exception as _e:
        import traceback; traceback.print_exc(); print("CLOTH JOBS FAILED, keeping the Rust cloth:", repr(_e))
specs = {n["name"]: n for n in sidecar["nodes"]}
from infinigen.core.util.math import FixedSeed, int_hash
import importlib
from infinigen.assets.materials import ceramic, wood, fabric, metal, plastic, terrain, art, lamp_shaders, text
mods = {"ceramic": ceramic, "wood": wood, "fabric": fabric, "metal": metal, "plastic": plastic, "terrain": terrain, "art": art, "lamp_shaders": lamp_shaders, "text": text}
cache = {}
clay = None
if args.clay:
    clay = bpy.data.materials.new("clay"); clay.use_nodes = True
    clay.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.6, 0.6, 0.6, 1)
    clay.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.9
for o in list(bpy.data.objects):
    if o.type != "MESH": continue
    if clay is not None:
        o.data.materials.clear(); o.data.materials.append(clay); continue
    key = o.name.split(".")[0] if o.name not in specs else o.name
    nm = o.name if o.name in specs else next((k for k in specs if o.name.startswith(k)), None)
    if nm is None: continue
    spec = specs[nm].get("spec")
    if not spec: continue
    ck = json.dumps(spec, sort_keys=True)
    if ck not in cache:
        mod, cls = spec["cls"].split(".")
        with FixedSeed(int(spec.get("seed", 0))):
            try:
                if mod == "lamp_shaders" and cls.startswith("shader_"):
                    from infinigen.core import surface as _surf
                    cache[ck] = _surf.shaderfunc_to_material(getattr(lamp_shaders, cls))
                elif mod == "window" and cls == "glass":
                    from infinigen.assets.objects.windows.window import shader_window_glass
                    from infinigen.core import surface as _surf
                    cache[ck] = _surf.shaderfunc_to_material(shader_window_glass)
                else:
                    inst = getattr(mods[mod], cls)()
                    cache[ck] = inst(**spec.get("kwargs", {}))
            except Exception as e:
                print("MATERIAL FAIL", spec, repr(e)); cache[ck] = None
    if cache[ck] is not None:
        o.data.materials.clear(); o.data.materials.append(cache[ck])
sc = bpy.context.scene
# lighting: Infinigen's sky (same seed convention as the reference)
from infinigen.core import init
import infinigen_examples.generate_indoors
init.apply_gin_configs(configs=["base_indoors.gin", "fast_solve.gin", "singleroom.gin"], overrides=["compose_indoors.terrain_enabled=False"],
    config_folders=["infinigen_examples/configs_indoor", "infinigen_examples/configs_nature"])
w = bpy.data.worlds.new("World"); w.use_nodes = True; bpy.context.scene.world = w
try:
    if args.top or args.showcase: raise RuntimeError('skip sky')
    from infinigen.assets import lighting
    with FixedSeed(int_hash((args.seed, "sky_lighting"))):
        lighting.sky_lighting.add_lighting()
except Exception as e:
    print("SKY FAIL", repr(e))
if args.top:
    for o in list(bpy.data.objects):
        n = o.name.lower()
        if "ceiling" in n or "light" in n and "ceilinglight" in n: o.hide_render = True
    meshes = [o for o in bpy.data.objects if o.type == "MESH" and not o.hide_render]
    pts = [o.matrix_world @ mathutils.Vector(c) for o in meshes if "floor" in o.name.lower() for c in o.bound_box] or [o.matrix_world @ mathutils.Vector(c) for o in meshes for c in o.bound_box]
    x0 = min(p.x for p in pts); x1 = max(p.x for p in pts); y0 = min(p.y for p in pts); y1 = max(p.y for p in pts)
    cam = bpy.data.cameras.new("c"); cam.type = "ORTHO"; cam.ortho_scale = max(x1-x0, y1-y0) * 1.1
    co = bpy.data.objects.new("c", cam); sc.collection.objects.link(co)
    co.location = ((x0+x1)/2, (y0+y1)/2, 30); sc.camera = co
    sun = bpy.data.lights.new("s", "SUN"); sun.energy = 4
    so = bpy.data.objects.new("s", sun); sc.collection.objects.link(so); so.rotation_euler = (0.3, 0.2, 0)
    w = bpy.data.worlds.new("w2"); w.use_nodes = True; w.node_tree.nodes["Background"].inputs[1].default_value = 1.0; sc.world = w
    sc.cycles.film_exposure = 3.0
    sc.render.engine = "CYCLES"; sc.cycles.samples = args.samples; sc.cycles.device = "CPU"; sc.cycles.use_denoising = True
    sc.render.resolution_x = sc.render.resolution_y = 640; sc.render.filepath = args.out
    bpy.ops.render.render(write_still=True); sys.exit(0)
if args.showcase:
    meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    pts = [o.matrix_world @ mathutils.Vector(c) for o in meshes for c in o.bound_box]
    x0 = min(p.x for p in pts); x1 = max(p.x for p in pts); zh = max(p.z for p in pts)
    cam = bpy.data.cameras.new("c"); co = bpy.data.objects.new("c", cam); sc.collection.objects.link(co); sc.camera = co
    cam.lens = 35; dist = max((x1 - x0) * 1.1, 2.0) * 1.15
    co.location = ((x0 + x1) / 2, -dist, max(zh * 0.5, 0.6)); co.rotation_euler = (1.45, 0, 0)
    sun = bpy.data.lights.new("s", "SUN"); sun.energy = 3
    so = bpy.data.objects.new("s", sun); sc.collection.objects.link(so); so.rotation_euler = (0.9, 0.2, 0.4)
    w = bpy.data.worlds.new("w3"); w.use_nodes = True; w.node_tree.nodes["Background"].inputs[1].default_value = 1.0; sc.world = w
    sc.render.engine = "CYCLES"; sc.cycles.samples = args.samples; sc.cycles.device = "CPU"; sc.cycles.use_denoising = True
    sc.render.resolution_x, sc.render.resolution_y = 1200, 500; sc.render.filepath = args.out
    bpy.ops.render.render(write_still=True); sys.exit(0)
# point lights (PointLampFactory)
for L in sidecar.get("lights", []):
    ld = bpy.data.lights.new(L["name"], "POINT"); ld.energy = L["watt"] * args.light_scale; ld.shadow_soft_size = L["radius"]
    ld.use_nodes = True
    nt = ld.node_tree; nt.nodes.clear()
    bb = nt.nodes.new("ShaderNodeBlackbody"); bb.inputs["Temperature"].default_value = L["temperature"]
    em = nt.nodes.new("ShaderNodeEmission"); out = nt.nodes.new("ShaderNodeOutputLight")
    nt.links.new(bb.outputs[0], em.inputs["Color"]); nt.links.new(em.outputs[0], out.inputs[0])
    lo = bpy.data.objects.new(L["name"], ld); sc.collection.objects.link(lo)
    lo.location = mathutils.Vector(L["loc"])  # z-up coords match after glTF import (importer converts the glb; lights are added in blender space)

def view_camera(sc, room, view):
    """cornerA / cornerB views inside `room`: farthest floor corners from the centroid; skips corners whose camera spot is blocked"""
    import bpy, mathutils, math
    fl = [o for o in bpy.data.objects if o.type == "MESH" and o.name.startswith(room) and ".floor" in o.name]
    pts = [o.matrix_world @ v.co for o in fl for v in o.data.vertices]
    pts = [mathutils.Vector((p.x, p.y, 0)) for p in pts]
    cen = sum(pts, mathutils.Vector()) / len(pts)
    # blockers: every visible object except the room shell / window frames+glass (curtains and doors DO block the camera)
    skip = (".floor", ".wall", ".ceiling", "skirting", "exterior", "entrance", "placeholder", "vc", "portal")
    def is_frame(n): return ("window" in n and "curtain" not in n and "shutter" not in n and "rod" not in n)
    boxes = []
    for o in bpy.data.objects:
        if o.type != "MESH" or o.hide_render or any(k in o.name.lower() for k in skip) or is_frame(o.name.lower()): continue
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
    if view == "cornerB": cands = cands[1:] + cands[:1]
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
    print("VIEWCAND", [(a, b, round(c, 2)) for a, b, c, _ in scored])
    good = [x for x in scored if x[0] and x[1] >= 8]
    far = good[0][3] if good else max(scored, key=lambda x: (x[1] >= 7, min(x[2], 0.6), x[1]))[3]
    d = (cen - far); d.normalize()
    loc = far + d * 0.45; loc.z = 1.5
    tgt = mathutils.Vector((cen.x, cen.y, 0.9))
    cam = bpy.data.cameras.new("vc"); cam.lens = 18; cam.sensor_width = 36
    co = bpy.data.objects.new("vc", cam); sc.collection.objects.link(co)
    co.location = loc
    co.rotation_euler = (tgt - loc).to_track_quat("-Z", "Y").to_euler()
    sc.camera = co
# light portals at windows (the reference adds one is_portal area light per window; they steer sky light through windows)
import re as _re
for _o in list(bpy.data.objects):
    if _o.type == "MESH" and _re.fullmatch(r"window\d+", _o.name):
        _bb = [_o.matrix_world @ mathutils.Vector(c) for c in _o.bound_box]
        _lo = mathutils.Vector((min(b.x for b in _bb), min(b.y for b in _bb), min(b.z for b in _bb)))
        _hi = mathutils.Vector((max(b.x for b in _bb), max(b.y for b in _bb), max(b.z for b in _bb)))
        _d = _hi - _lo; _c = (_lo + _hi) / 2
        if _d.x < _d.y: _n, _u, _w = mathutils.Vector((1, 0, 0)), mathutils.Vector((0, 1, 0)), _d.y
        else: _n, _u, _w = mathutils.Vector((0, 1, 0)), mathutils.Vector((-1, 0, 0)), _d.x
        _v = mathutils.Vector((0, 0, 1))
        _m = mathutils.Matrix(((_u.x, _v.x, _n.x), (_u.y, _v.y, _n.y), (_u.z, _v.z, _n.z))).to_4x4(); _m.translation = _c
        _ld = bpy.data.lights.new("portal", "AREA"); _ld.size = 1; _ld.size_y = 1; _ld.cycles.is_portal = True
        _po = bpy.data.objects.new("portal", _ld); sc.collection.objects.link(_po)
        _po.matrix_world = _m; _po.scale = (_w, _d.z, 1)
# camera
if args.view in ("cornerA", "cornerB"):
    view_camera(sc, args.room, args.view); co = sc.camera; cam = co.data
else:
  cam = bpy.data.cameras.new("cam");   co = bpy.data.objects.new("cam", cam); sc.collection.objects.link(co); sc.camera = co
  if args.cam:
    c = json.load(open(args.cam))
    co.matrix_world = mathutils.Matrix(c["matrix"]); cam.lens = c.get("lens", 15); cam.sensor_width = c.get("sensor_width", 36)
sc.cycles.film_exposure = 3.0
sc.render.engine = "CYCLES"; sc.cycles.samples = args.samples; sc.cycles.device = "CPU"; sc.cycles.use_denoising = True
sc.render.resolution_x, sc.render.resolution_y = 960, 540
sc.render.filepath = args.out
if os.environ.get('DEBUGL'):
    from collections import Counter
    print('LIGHTS', Counter((o.data.type, round(o.data.energy,1), getattr(o.data.cycles,'is_portal',False)) for o in bpy.data.objects if o.type=='LIGHT'))
    for n in sc.world.node_tree.nodes:
        if n.type=='BACKGROUND': print('BG', n.inputs[1].default_value)
        if n.type=='TEX_SKY': print('SKY', n.sun_elevation, n.sun_rotation, n.air_density, n.dust_density, getattr(n,'sun_intensity',None))
    print('VIEW', sc.view_settings.view_transform, sc.view_settings.exposure, sc.cycles.film_exposure)
if args.norender: print('NORENDER done'); sys.exit(0)
t = time.time(); bpy.ops.render.render(write_still=True); print("render time", time.time() - t)

"""Quick people preview (Blender 5.2): ~/work/infinigen-ref/.venv52/bin/python tools/people/preview.py X.glb out.png
 [--cam x,y,z,tx,ty,tz] [--lens 50] [--samples 24] [--res 1200x700] [--exposure 0]"""
import sys, os, json, math, argparse
sys.path.insert(0, os.path.expanduser("~/work/ref/scripts"))
import bpy, mathutils
ap = argparse.ArgumentParser(); ap.add_argument("glb"); ap.add_argument("out")
ap.add_argument("--cam"); ap.add_argument("--lens", type=float, default=50); ap.add_argument("--samples", type=int, default=24)
ap.add_argument("--res", default="1200x700"); ap.add_argument("--exposure", type=float, default=0.0); ap.add_argument("--sunstr", type=float, default=3.0)
a = ap.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:])
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=a.glb)
side = json.load(open(a.glb[:-4] + ".json"))
specs = {n["name"]: n.get("spec") for n in side["nodes"]}
import people_mats
cache = {}
for o in list(bpy.data.objects):
    if o.type != "MESH": continue
    nm = o.name if o.name in specs else next((k for k in specs if o.name.startswith(k)), None)
    sp = specs.get(nm) if nm else None
    if sp and sp["cls"].startswith("people."):
        k = json.dumps(sp, sort_keys=True)
        if k not in cache: cache[k] = people_mats.make(sp)
        o.data.materials.clear(); o.data.materials.append(cache[k])
    elif sp is None or True:
        pass
sc = bpy.context.scene
pts = [o.matrix_world @ mathutils.Vector(c) for o in bpy.data.objects if o.type == "MESH" and o.name.startswith("person") for c in o.bound_box]
x0 = min(p.x for p in pts); x1 = max(p.x for p in pts); y0 = min(p.y for p in pts); y1 = max(p.y for p in pts); z1 = max(p.z for p in pts)
cam = bpy.data.cameras.new("c"); co = bpy.data.objects.new("c", cam); sc.collection.objects.link(co); sc.camera = co
cam.lens = a.lens
if a.cam:
    v = [float(x) for x in a.cam.split(",")]
    loc = mathutils.Vector(v[:3]); tgt = mathutils.Vector(v[3:6])
else:
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    cam.lens = 35
    D = max((x1 - x0) * 0.98 + 0.4, 2.6) + (y1 - y0) * 0.9
    loc = mathutils.Vector((cx + (x1 - x0) * 0.0, y1 + D, 1.25 + (y1 - y0) * 0.3)); tgt = mathutils.Vector((cx, cy, 0.85))
co.location = loc; co.rotation_euler = (tgt - loc).to_track_quat("-Z", "Y").to_euler()
sun = bpy.data.lights.new("s", "SUN"); sun.energy = a.sunstr; sun.angle = math.radians(5)
so = bpy.data.objects.new("s", sun); sc.collection.objects.link(so); so.rotation_euler = (math.radians(50), 0, math.radians(-25))
w = bpy.data.worlds.new("w"); w.use_nodes = True; w.node_tree.nodes["Background"].inputs[1].default_value = 0.6; w.node_tree.nodes["Background"].inputs[0].default_value = (0.75, 0.82, 1.0, 1); sc.world = w
sc.render.engine = "CYCLES"; sc.cycles.samples = a.samples; sc.cycles.device = "CPU"; sc.cycles.use_denoising = True
sc.view_settings.view_transform = "AgX" if "AgX" in [x.identifier for x in sc.view_settings.bl_rna.properties["view_transform"].enum_items] else "Standard"
sc.view_settings.exposure = a.exposure
rx, ry = a.res.split("x"); sc.render.resolution_x = int(rx); sc.render.resolution_y = int(ry)
sc.render.filepath = a.out
bpy.ops.render.render(write_still=True)

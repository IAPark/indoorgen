"""bedref.py SEED OUTDIR [--no-blender]: Blender's cloth solver on a bed's jobs (the reference) against the procedural bedding (tools: target/fast/beddump).
Prints per cloth part: how far it reaches past the mattress on each side, its top height and the surface distance to the reference;
renders OUTDIR/SEED.png, Cycles clay, reference left, procedural right. The reference is cached (OUTDIR/SEED/blend.npz)."""
import sys, os, json, subprocess, numpy as np, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
seed, out = sys.argv[1], sys.argv[2]
root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
d = os.path.join(out, seed); os.makedirs(d, exist_ok=True)
env = dict(os.environ)
env.pop("BED_SIM", None)
subprocess.run([f"{root}/target/fast/beddump", seed, f"{d}/proc"], env=env, check=True, capture_output=True)
def load(t):
    p = {}
    for k in range(40):
        f = f"{d}/{t}/part.{k}.json"
        if os.path.exists(f):
            j = json.load(open(f)); p[k] = (np.array(j["verts"], dtype=float).reshape(-1, 3), j["polys"])
    return p
import bpy
cache = f"{d}/blend.json"
if not os.path.exists(cache):
    env["BED_SIM"] = "1"
    subprocess.run([f"{root}/target/fast/beddump", seed, f"{d}/sim"], env=env, check=True, capture_output=True)
    import cloth_jobs
    jobs = json.load(open(f"{d}/sim/jobs.json"))
    parts = {f"bed#{k}": v for k, v in load("sim").items()}
    bpy.ops.wm.read_factory_settings(use_empty=True)
    cloth_jobs.run_jobs(jobs, lambda n: parts[n], lambda n, v, f: parts.__setitem__(n, (np.asarray(v), f)), verbose=False)
    json.dump({"targets": [j["target"] for j in jobs if j["kind"] == "cloth"], "parts": {k: {"verts": np.asarray(v[0]).tolist(), "polys": [list(map(int, x)) for x in v[1]]} for k, v in parts.items()}}, open(cache, "w"))
c = json.load(open(cache))
ref = {int(k.split("#")[1]): (np.array(v["verts"]), v["polys"]) for k, v in c["parts"].items()}
proc = load("proc")
targets = [int(str(t).split("#")[1]) for t in c["targets"]]
from scipy.spatial import cKDTree
cloths = targets[-2:]; mat_i = cloths[0] - 1
ml, mh = ref[mat_i][0].min(0), ref[mat_i][0].max(0)
def reach(v):
    lo, hi = v.min(0), v.max(0)
    return "head %+.2f side %+.2f/%+.2f foot %+.2f top %+.3f low %+.2f" % (ml[0] - lo[0], ml[1] - lo[1], hi[1] - mh[1], hi[0] - mh[0], hi[2] - mh[2], lo[2])
for name, k in zip(("sheet", "cover"), cloths):
    a, b = ref[k][0], proc[k][0]
    d1, d2 = cKDTree(a).query(b)[0], cKDTree(b).query(a)[0]
    print(f"{seed} {name} ref  {reach(a)}"); print(f"{seed} {name} proc {reach(b)}  dist mean {max(d1.mean(), d2.mean()):.3f} p95 {max(np.percentile(d1, 95), np.percentile(d2, 95)):.3f}")
if "--no-render" in sys.argv: sys.exit(0)
bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
sc.render.engine = "CYCLES"; sc.cycles.samples = 24; sc.cycles.use_denoising = False
sc.render.resolution_x, sc.render.resolution_y = 1100, 520
w = bpy.data.worlds.new("w"); w.use_nodes = True; w.node_tree.nodes["Background"].inputs[0].default_value = (0.8, 0.8, 0.85, 1); w.node_tree.nodes["Background"].inputs[1].default_value = 0.5; sc.world = w
gap = 3.2
def add(parts, xo, ):
    for k, (v, f) in parts.items():
        me = bpy.data.meshes.new(f"m{k}"); me.from_pydata([tuple(p) for p in v], [], [list(map(int, x)) for x in f]); me.update()
        for p in me.polygons: p.use_smooth = True
        o = bpy.data.objects.new(f"o{k}", me); o.location = (0, xo, 0); sc.collection.objects.link(o)
add(ref, -gap / 2); add(proc, gap / 2)
floor_z = min(ref[k][0][:, 2].min() for k in ref)
bpy.ops.mesh.primitive_plane_add(size=30, location=(1, 0, floor_z))
s = bpy.data.lights.new("sun", "SUN"); s.energy = 3.0; so = bpy.data.objects.new("sun", s); so.rotation_euler = (math.radians(50), math.radians(-25), math.radians(20)); sc.collection.objects.link(so)
cam = bpy.data.cameras.new("c"); cam.lens = 38; co = bpy.data.objects.new("c", cam); sc.collection.objects.link(co); sc.camera = co
import mathutils
co.location = (6.6, 1.8, 3.0)
co.rotation_euler = (mathutils.Vector((1.0, 0, 0.2)) - co.location).to_track_quat("-Z", "Y").to_euler()
sc.render.filepath = os.path.abspath(f"{out}/{seed}.png")
bpy.ops.render.render(write_still=True)

"""pyop.py in.json out.json : run python populate_doors/populate_windows on the cutters of an opdump json (same stage seeds), dump world meshes per cutter"""
import sys, json, numpy as np, bpy, types
from mathutils import Matrix
d = json.load(open(sys.argv[1])); out = sys.argv[2]
from infinigen.core.constraints.constraint_language.constants import RoomConstants
from infinigen.core.constraints.example_solver.room import decorate
from infinigen.core.util.math import FixedSeed, int_hash
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(0)
C = RoomConstants()
for k, v in d["consts"].items():
    setattr(C, k, v)
seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
only = sys.argv[4] if len(sys.argv) > 4 else "both"
def cutter(ci, c):
    bpy.ops.mesh.primitive_cube_add(size=1)
    o = bpy.context.active_object
    o.name = "cutter%d" % ci
    o.scale = tuple(c["dims"]); bpy.ops.object.transform_apply(scale=True)
    o.location = tuple(c["loc"]); o.rotation_euler = (0, 0, c["rot_z"])
    return o
dc, wc = [], []
state = types.SimpleNamespace(objs={})
for ci, c in enumerate(d["cutters"]):
    o = cutter(ci, c)
    (wc if c["kind"] == "window" else dc).append((ci, o))
    state.objs[o.name] = types.SimpleNamespace(relations=[types.SimpleNamespace(target_name=c["rooms"][0])])
bpy.context.view_layer.update()
res = {}
def collect(tag, plist):
    for ci, o in plist:
        ms = []
        for ch in o.children_recursive:
            if ch.type != "MESH": continue
            dg = bpy.context.evaluated_depsgraph_get(); e = ch.evaluated_get(dg).to_mesh(); mw = np.array(ch.matrix_world)
            V = [[float(x) for x in (mw @ np.append(np.array(v.co), 1.0))[:3]] for v in e.vertices]
            ms.append({"name": ch.name, "verts": V, "polys": [[int(i) for i in p.vertices] for p in e.polygons]})
        res["%s%d" % (tag, ci)] = ms
        res["%s%d_rot" % (tag, ci)] = [[float(ch.rotation_euler[2]), type(ch).__name__, ch.name] for ch in o.children]
if only in ("both", "doors"):
    with FixedSeed(int_hash((seed, "room_doors"))):
        decorate.populate_doors([o for _, o in dc], C)
    bpy.context.view_layer.update(); collect("door", dc)
if only in ("both", "windows"):
    with FixedSeed(int_hash((seed, "room_windows"))):
        decorate.populate_windows([o for _, o in wc], C, state)
    bpy.context.view_layer.update(); collect("window", wc)
json.dump(res, open(out, "w")); print("PYOP", {k: len(v) for k, v in res.items()})

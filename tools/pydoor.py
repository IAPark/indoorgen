"""pydoor.py Class seed const_seed out.json : python door with RoomConstants drawn from np.random.seed(const_seed); dumps params + intermediate meshes"""
import sys, json, numpy as np, bpy, importlib
cls, seed, cseed, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
from infinigen.core.constraints.constraint_language.constants import RoomConstants
from infinigen.assets.objects.elements.doors import panel, lite, louver, base
from infinigen.core.util import blender as butil
M = {"PanelDoorFactory": panel.PanelDoorFactory, "GlassPanelDoorFactory": panel.GlassPanelDoorFactory, "LouverDoorFactory": louver.LouverDoorFactory, "LiteDoorFactory": lite.LiteDoorFactory}
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(cseed); consts = RoomConstants()
cap = {}
orig = butil.group_in_collection
def gic(objs, name, **k):
    cap["objs"] = objs; return orig(objs, name, **k)
butil.group_in_collection = gic
np.random.seed(12345)
f = M[cls](seed, constants=consts)
def mesh(o):
    if o is None: return None
    m = o.data; mw = np.array(o.matrix_world)
    return {"verts": [[float(x) for x in (mw @ np.append(np.array(v.co), 1.0))[:3]] for v in m.vertices], "polys": [[int(i) for i in p.vertices] for p in m.polygons]}
o = f.spawn_asset(0)
bpy.context.view_layer.update()
names = ["door", "handle", "frame", "arc"]
parts = {n: mesh(ob) for n, ob in zip(names, cap["objs"])}
dg = bpy.context.evaluated_depsgraph_get(); e = o.evaluated_get(dg).to_mesh()
mw = np.array(o.matrix_world)
pid = [int(a.data[i].value) for a in [e.attributes["part_id"]] for i in range(len(e.vertices))] if "part_id" in e.attributes else []
attrs = {n: [int(e.attributes[n].data[i].value) for i in range(len(e.vertices))] for n in ("door","door_frame","door_left","door_right","handle","axis_group") if n in e.attributes}
final = {"part_id": pid, "attrs": attrs, "verts": [[float(x) for x in (mw @ np.append(np.array(v.co), 1.0))[:3]] for v in e.vertices], "polys": [[int(i) for i in p.vertices] for p in e.polygons]}
def cv(v):
    if isinstance(v, (np.floating,)): return float(v)
    if isinstance(v, (np.integer,)): return int(v)
    if isinstance(v, (np.bool_,)): return bool(v)
    if isinstance(v, (np.str_,)): return str(v)
    if isinstance(v, (list, tuple, np.ndarray)):
        try: return [cv(x) for x in v]
        except Exception: return None
    if isinstance(v, (int, float, str, bool)) or v is None: return v
    return None
params = {k: cv(v) for k, v in vars(f).items() if k not in ("constants", "surface", "auto_bevel")}
params["bevel_amount"] = float(f.auto_bevel.amount); params["bevel_segments"] = int(f.auto_bevel.segments)
json.dump({"params": params, "consts": {"width": consts.door_width, "size": consts.door_size, "wall": consts.wall_thickness}, "parts": parts, "final": final}, open(out, "w"))
print("PYDOOR", cls, len(final["verts"]), {k: (len(v["verts"]) if v else 0) for k, v in parts.items()})

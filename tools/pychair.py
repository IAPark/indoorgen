"""pychair.py module.Class seed out.json : dump every part passed to join_objects in ChairFactory.create_asset (world co, polys), plus params"""
import sys, json, numpy as np, bpy, importlib
mod, cls = sys.argv[1].rsplit(".", 1); seed = int(sys.argv[2]); out = sys.argv[3]
bpy.ops.wm.read_factory_settings(use_empty=True)
import infinigen.core.util.random as R
class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k): return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()
import os
if not os.environ.get('NOSTUB'): R.weighted_sample = lambda *a, **k: _D
import infinigen.core.surface as S
S.assign_material = lambda *a, **k: None
C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
chmod = importlib.import_module("infinigen.assets.objects.seating.chairs.chair")
parts = {"list": []}
orig = chmod.join_objects
def snap(o):
    dg = bpy.context.evaluated_depsgraph_get(); e = o.evaluated_get(dg).to_mesh(); mw = np.array(o.matrix_world)
    V = [[float(x) for x in (mw @ np.append(np.array(v.co), 1.0))[:3]] for v in e.vertices]
    return {"verts": V, "polys": [[int(i) for i in p.vertices] for p in e.polygons], "name": o.name}
def jo(objs):
    if isinstance(objs, list) and len(objs) > 0 and getattr(objs[0], "type", "") == "MESH" and len(objs) >= 3:
        parts["list"].append([snap(o) for o in objs])
    return orig(objs)
chmod.join_objects = jo
np.random.seed(seed)
f = C(seed)
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
params = {k: cv(v) for k, v in vars(f).items() if k not in ("surface", "limb_surface", "panel_surface", "scratch", "edge_wear", "clothes_scatter", "surface_material_gen", "limb_surface_material_gen")}
o = f.spawn_asset(0)
bpy.context.view_layer.update()
fin = snap(o)
json.dump({"params": params, "parts": parts["list"][-1] if parts["list"] else [], "final": fin}, open(out, "w"))
print("PYCHAIR", len(parts["list"][-1]) if parts["list"] else 0, len(fin["verts"]))

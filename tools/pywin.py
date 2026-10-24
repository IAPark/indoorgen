"""pywin.py seed w h t out.json : python WindowFactory(seed).spawn_asset(0, dimensions=(w,h,t), open=False); dumps params + mesh"""
import sys, json, numpy as np, bpy
from infinigen.assets.objects.windows import window
seed = int(sys.argv[1]); dims = tuple(float(x) for x in sys.argv[2:5]); out = sys.argv[5]
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(999)
f = window.WindowFactory(seed)
bev = float(f.beveler.amount); seg = int(f.beveler.segments)
o = f.spawn_asset(0, dimensions=dims, open=False)
bpy.context.view_layer.update()
dg = bpy.context.evaluated_depsgraph_get(); ob = [x for x in [o] + list(o.children_recursive) if x.type == "MESH"][0]
e = ob.evaluated_get(dg).to_mesh(); mw = np.array(ob.matrix_world)
V = [[float(x) for x in (mw @ np.append(np.array(v.co), 1.0))[:3]] for v in e.vertices]
P = [[int(i) for i in p.vertices] for p in e.polygons]
def cv(v):
    if isinstance(v, (np.floating,)): return float(v)
    if isinstance(v, (np.integer,)): return int(v)
    if isinstance(v, (np.bool_, bool)): return bool(v)
    if isinstance(v, (int, float, str)): return v
    return None
params = {k: cv(v) for k, v in f.params.items()}
json.dump({"params": params, "bev": bev, "seg": seg, "verts": V, "polys": P}, open(out, "w")); print("PYWIN", len(V), len(P), bev)

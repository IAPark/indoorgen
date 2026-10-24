"""pydump.py module.Class seed out.json : dump python factory asset (materials stubbed like geomcmp) as json verts/polys (world space)."""
import sys, json, importlib, numpy as np, bpy
class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k): return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()
import infinigen.core.util.random as R
R.weighted_sample = lambda *a, **k: _D
import infinigen.core.surface as S
S.assign_material = lambda *a, **k: None
a = sys.argv[sys.argv.index("--")+1:] if "--" in sys.argv else sys.argv[1:]
mod, cls = a[0].rsplit(".", 1); seed = int(a[1]); out = a[2]
C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(seed)
f = C(seed); o = f.spawn_asset(0); bpy.context.view_layer.update()
dg = bpy.context.evaluated_depsgraph_get()
V = []; P = []; off = 0
for ob in [o] + list(o.children_recursive):
    if ob.type != "MESH": continue
    e = ob.evaluated_get(dg); m = e.to_mesh()
    mw = np.array(ob.matrix_world)
    for v in m.vertices:
        c = np.array(mw @ np.append(np.array(v.co), 1.0))[:3]; V.append([float(x) for x in c])
    for p in m.polygons: P.append([int(i) + off for i in p.vertices])
    off += len(m.vertices); e.to_mesh_clear()
json.dump({"verts": V, "polys": P}, open(out, "w"))
print("PYDUMP", len(V), len(P))

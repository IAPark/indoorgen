"""pybedfull.py seed out_prefix : BedFactory(seed).spawn_asset(0) with every object (frame + children) dumped separately as out_prefix.<k>_<name>.json (world space)"""
import sys, json, numpy as np, bpy
a = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
seed = int(a[0]); pre = a[1]
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
from infinigen.assets.objects.seating import bed
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(seed)
f = bed.BedFactory(seed)
obj = f.spawn_asset(0)
dg = bpy.context.evaluated_depsgraph_get()
objs = [obj] + list(obj.children)
for k, o in enumerate(objs):
    e = o.evaluated_get(dg); me = e.to_mesh()
    V = [list(o.matrix_world @ v.co) for v in me.vertices]; P = [list(p.vertices) for p in me.polygons]
    json.dump({"verts": V, "polys": P}, open(f"{pre}.{k}.json", "w")); print(k, o.name, len(V), len(P))
    e.to_mesh_clear()

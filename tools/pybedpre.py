"""pybedfull.py seed out_prefix : BedFactory(seed).spawn_asset(0) with every object (frame + children) dumped separately as out_prefix.<k>_<name>.json (world space)"""
import os, sys, json, numpy as np, bpy
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
import infinigen.assets.scatters.clothes as CL
_orig = CL.cloth_sim
_k = [0]
def _dump(o, name):
    me = o.data
    json.dump({"verts": [list(o.matrix_world @ v.co) for v in me.vertices], "polys": [list(p.vertices) for p in me.polygons]}, open(f"{pre}.{name}.json", "w"))
def wrapped(clothes, obj=None, end_frame=50, **kw):
    k = _k[0]; _k[0] += 1
    _dump(clothes, f"pre{k}")
    for i, o in enumerate(obj if isinstance(obj, (list, tuple)) else ([] if obj is None else [obj])):
        _dump(o, f"col{k}_{i}")
    print("PRESIM", k, end_frame, kw)
    cols = list(obj) if obj is not None else []
    if k == 1:
        sys.path.insert(0, "tools")
        import cloth_jobs
        vs = [tuple(clothes.matrix_world @ v.co) for v in clothes.data.vertices]; fs = [list(p.vertices) for p in clothes.data.polygons]
        cc = [cloth_jobs._make("cc", [tuple(o.matrix_world @ v.co) for v in o.data.vertices], [list(p.vertices) for p in o.data.polygons]) for o in cols]
        print("MODS", [[m.type for m in o.modifiers] for o in cols], clothes.modifiers[:], [tuple(o.scale) for o in cols], [tuple(o.location) for o in cols], tuple(clothes.location), [o.parent for o in cols])
        mine = cloth_jobs._make("mine", vs, fs)
        if os.environ.get("USEEDGES"):
            ed = [tuple(e.vertices) for e in clothes.data.edges]
            me2 = bpy.data.meshes.new("m2"); me2.from_pydata(vs, ed, fs); me2.update(); mine.data = me2
            json.dump({"edges": ed}, open(f"{pre}.edges{k}.json", "w"))
        if os.environ.get("USECOPY"):
            mine.data = clothes.data.copy()
        print("EDGES", len(clothes.data.edges), len(mine.data.edges), list(clothes.data.edges[0].vertices), list(mine.data.edges[0].vertices))
        _orig(mine, cc, end_frame, **kw)
        import numpy as _np
        mv = _np.array([tuple(v.co) for v in mine.data.vertices])
        r = _orig(clothes, obj, end_frame, **kw)
        rv = _np.array([tuple(v.co) for v in clothes.data.vertices])
        print("INSESSION", _np.abs(mv - rv).max(), _np.abs(mv - rv).mean())
        return r
    return _orig(clothes, obj, end_frame, **kw)
CL.cloth_sim = wrapped
bed.clothes.cloth_sim = wrapped
import infinigen.assets.objects.seating.mattress as MT, infinigen.assets.objects.seating.pillow as PL
MT.clothes.cloth_sim = wrapped; PL.clothes.cloth_sim = wrapped
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(seed)
f = bed.BedFactory(seed)
obj = f.spawn_asset(0)
dg = bpy.context.evaluated_depsgraph_get()
for k, o in enumerate([obj] + list(obj.children)):
    e = o.evaluated_get(dg); me = e.to_mesh()
    json.dump({"verts": [list(o.matrix_world @ v.co) for v in me.vertices], "polys": [list(p.vertices) for p in me.polygons]}, open(f"{pre}.final{k}.json", "w")); e.to_mesh_clear()

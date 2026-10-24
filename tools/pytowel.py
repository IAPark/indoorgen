"""pytowel.py seed out.json : TowelFactory(seed).spawn_asset(0) with snapshots after each modifier / method"""
import sys, json, numpy as np, bpy
seed = int(sys.argv[1]); out = sys.argv[2]
bpy.ops.wm.read_factory_settings(use_empty=True)
import infinigen.core.util.random as R
class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k): return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()
R.weighted_sample = lambda *a, **k: _D
import infinigen.core.surface as S
S.assign_material = lambda *a, **k: None
from infinigen.assets.objects.clothes import towel as T
from infinigen.core.util import blender as butil
from infinigen.assets.utils import uv as UV
T.wrap_sides = lambda obj, mat, *a, **k: None
snaps = []
def snap(o, tag):
    bpy.context.view_layer.update()
    if len(o.modifiers) == 0:
        e = o.data
    else:
        dg = bpy.context.evaluated_depsgraph_get(); e = o.evaluated_get(dg).to_mesh()
    snaps.append({"tag": tag, "verts": [[float(x) for x in v.co] for v in e.vertices], "polys": [[int(i) for i in p.vertices] for p in e.polygons]})
om = butil.modify_mesh
def mm(obj, type, *a, **k):
    r = om(obj, type, *a, **k); snap(obj, "mod_" + type); return r
T.butil.modify_mesh = mm
for name in ("fold", "pre_roll", "roll"):
    f = getattr(T.TowelFactory, name)
    def wrap(f=f, name=name):
        def g(self, obj, *a, **k):
            r = f(self, obj, *a, **k); snap(obj, name); return r
        return g
    setattr(T.TowelFactory, name, wrap())
oag = S.add_geomod
def ag(objs, fn, *a, **k):
    r = oag(objs, fn, *a, **k)
    o = objs if not isinstance(objs, list) else objs[0]
    snap(o, "geomod_" + fn.__name__); return r
T.surface.add_geomod = ag
np.random.seed(seed)
f = T.TowelFactory(seed)
params = {k: (float(v) if isinstance(v, (float, np.floating)) else (int(v) if isinstance(v, (int, np.integer)) else (str(v) if isinstance(v, (str, np.str_)) else None))) for k, v in vars(f).items() if k not in ("surface", "surface_material_gen")}
o = f.spawn_asset(0)
snap(o, "final")
json.dump({"params": params, "snaps": snaps}, open(out, "w"))
print("PYTOWEL", [(s["tag"], len(s["verts"])) for s in snaps], params)

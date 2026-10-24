"""pybedparts.py seed out_prefix : python BedFrameFactory(seed).spawn_asset(0) with each make_* part dumped as out_prefix.<name>.json
(parts: seat, legs, backs, leg_decors, back_decors; materials stubbed; global np seed = seed before the ctor like refcache)."""
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
from infinigen.assets.objects.seating import bedframe
from infinigen.assets.utils.decorate import read_co
def dump(objs, name):
    if not isinstance(objs, list): objs = [objs]
    V = []; P = []
    for o in objs:
        dg = bpy.context.evaluated_depsgraph_get(); e = o.evaluated_get(dg); me = e.to_mesh()
        off = len(V)
        V += [list(o.matrix_world @ v.co) for v in me.vertices]
        P += [[off + i for i in p.vertices] for p in me.polygons]
        e.to_mesh_clear()
    json.dump({"verts": V, "polys": P}, open(f"{pre}.{name}.json", "w"))
    print(name, len(V), len(P))
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(seed)
C = bedframe.BedFrameFactory
for n in ["make_seat", "make_legs", "make_backs", "make_leg_decors", "make_back_decors"]:
    def mk(n):
        orig = getattr(C, n)
        def w(self, *args, **kw):
            r = orig(self, *args, **kw)
            dump(r, n)
            return r
        return w
    setattr(C, n, mk(n))
f = C(seed)
f.spawn_asset(0)

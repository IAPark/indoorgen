"""pybbox.py module.Class SEEDS : print vertex count + bbox of spawn_asset(0) (materials stubbed like refcache)"""
import sys, numpy as np, importlib, bpy
a = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
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
mod, cls = a[0].rsplit(".", 1)
C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
for s in [int(x) for x in a[1].split(",")]:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    np.random.seed(s); f = C(s); o = f.spawn_asset(0)
    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get(); V = []
    for ob in [o] + list(o.children_recursive):
        if ob.type != "MESH": continue
        e = ob.evaluated_get(dg); m = e.to_mesh(); n = len(m.vertices)
        co = np.empty(n * 3, dtype=np.float32); m.vertices.foreach_get("co", co); V.append(co.reshape(-1, 3)); e.to_mesh_clear()
    V = np.concatenate(V)
    print("BBOX", cls, s, len(V), np.round(V.min(0), 3), np.round(V.max(0), 3), type(getattr(f, "factory", None)).__name__)

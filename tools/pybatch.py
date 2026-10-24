"""pybatch.py module.Class seed0 N out.jsonl [inst] : dump spawn_asset(inst) for seeds seed0..seed0+N-1 (materials stubbed like geomcmp)"""
import sys, json, importlib, numpy as np, bpy
class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k): return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()
import infinigen.core.util.random as R
import os
if not os.environ.get('NOSTUB'): R.weighted_sample = lambda *a, **k: _D
import infinigen.core.surface as S
S.assign_material = lambda *a, **k: None
a = sys.argv[sys.argv.index("--")+1:] if "--" in sys.argv else sys.argv[1:]
mod, cls = a[0].rsplit(".", 1); s0 = int(a[1]); N = int(a[2]); out = a[3]; inst = int(a[4]) if len(a) > 4 else 0
C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
bpy.ops.wm.read_factory_settings(use_empty=True)
fo = open(out, "w")
for seed in range(s0, s0 + N):
    try:
        np.random.seed(seed)
        f = C(seed); o = (f.create_placeholder() if __import__('os').environ.get('PH') else f.spawn_asset(inst)); bpy.context.view_layer.update()
        dg = bpy.context.evaluated_depsgraph_get()
        V = []; P = []; off = 0
        for ob in [o] + list(o.children_recursive):
            if ob.type != "MESH": continue
            e = ob.evaluated_get(dg); m = e.to_mesh(); mw = np.array(ob.matrix_world)
            co = np.array([tuple(v.co) for v in m.vertices]).reshape(-1, 3)
            co = co @ mw[:3, :3].T + mw[:3, 3]
            V += co.tolist()
            for p in m.polygons: P.append([int(i) + off for i in p.vertices])
            off += len(m.vertices); e.to_mesh_clear()
        fo.write(json.dumps({"class": cls, "fseed": seed, "inst": inst, "verts": V, "polys": P}) + "\n")
        for ob in [o] + list(o.children_recursive): bpy.data.objects.remove(ob, do_unlink=True)
    except Exception as ex:
        import traceback; traceback.print_exc()
        fo.write(json.dumps({"class": cls, "fseed": seed, "inst": inst, "error": repr(ex)[:200]}) + "\n")
    fo.flush()
print("PYBATCH done")

"""pycontainer.py module.Class SEED INST OUTDIR : dump the parts joined by PlantContainerFactory.create_asset (pot, plant, dirt) as 90{0,1,2}_part_out.bin (world-space, scale/location applied)"""
import sys, os, numpy as np, importlib, struct, json
import bpy
sys.path.insert(0, "tools")
a = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
modcls, seed, inst, outdir = a[0], int(a[1]), int(a[2]), a[3]
os.makedirs(outdir, exist_ok=True)
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
import infinigen.assets.objects.tableware.plant_container as PC
orig = PC.join_objects
def dump(path, ob):
    m = ob.data; n = len(m.vertices)
    co = np.empty(n * 3, dtype=np.float32); m.vertices.foreach_get("co", co)
    mw = np.array(ob.matrix_basis, dtype=np.float64)
    co = (co.reshape(-1, 3).astype(np.float64) @ mw[:3, :3].T + mw[:3, 3]).astype(np.float32)
    ls = np.empty(len(m.polygons), dtype=np.uint32); m.polygons.foreach_get("loop_total", ls)
    lv = np.empty(len(m.loops), dtype=np.uint32); m.loops.foreach_get("vertex_index", lv)
    hb = json.dumps(dict(nv=int(n), nf=int(len(ls)), too_big=False)).encode()
    with open(path, "wb") as fh:
        fh.write(b"RCv1"); fh.write(struct.pack("<I", len(hb))); fh.write(hb)
        fh.write(co.tobytes()); fh.write(ls.astype("<u4").tobytes()); fh.write(lv.astype("<u4").tobytes())
def jo(objs):
    for k, o in enumerate(objs): dump(f"{outdir}/90{k}_part_out.bin", o)
    return orig(objs)
PC.join_objects = jo
mod, cls = modcls.rsplit(".", 1)
C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(seed)
f = C(seed)
f.spawn_asset(inst)
print("DONE")

"""pytrace.py: run a python factory and dump the mesh before/after every butil.modify_mesh / surface.add_geomod(apply) call.
usage: pytrace.py module.Class SEED INST OUTDIR [--filter SUBSTR]
Files: OUTDIR/NNN_<kind>_in.bin / _out.bin (RCv1 format as tools/refcache.py) + OUTDIR/NNN_<kind>.json (kwargs). Final asset: OUTDIR/final.bin.
Same conventions as refcache.py (np.random.seed(SEED) before ctor, weighted_sample stubbed, assign_material no-op)."""
import sys, os, json, struct, importlib
import numpy as np
import bpy

def dump_obj(path, ob, evaluated=False, extra=None):
    if evaluated:
        dg = bpy.context.evaluated_depsgraph_get(); e = ob.evaluated_get(dg); m = e.to_mesh()
    else:
        m = ob.data
    n = len(m.vertices)
    co = np.empty(n * 3, dtype=np.float32); m.vertices.foreach_get("co", co)
    ls = np.empty(len(m.polygons), dtype=np.uint32); m.polygons.foreach_get("loop_total", ls)
    lv = np.empty(len(m.loops), dtype=np.uint32); m.loops.foreach_get("vertex_index", lv)
    hdr = dict(nv=int(n), nf=int(len(ls)), too_big=False, **(extra or {}))
    hb = json.dumps(hdr).encode()
    with open(path, "wb") as fh:
        fh.write(b"RCv1"); fh.write(struct.pack("<I", len(hb))); fh.write(hb)
        fh.write(co.tobytes()); fh.write(ls.astype("<u4").tobytes()); fh.write(lv.astype("<u4").tobytes())
        ed = np.empty(len(m.edges) * 2, dtype=np.uint32); m.edges.foreach_get("vertices", ed)
        fh.write(b"EDG1"); fh.write(struct.pack("<I", len(m.edges))); fh.write(ed.astype("<u4").tobytes())
    if evaluated: e.to_mesh_clear()

class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k): return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()

def main():
    a = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    modcls, seed, inst, outdir = a[0], int(a[1]), int(a[2]), a[3]
    os.makedirs(outdir, exist_ok=True)
    for f in os.listdir(outdir): os.remove(os.path.join(outdir, f))
    counter = [0]
    import infinigen.core.util.blender as butil
    import infinigen.core.util.random as R
    R.weighted_sample = lambda *a, **k: _D
    import infinigen.core.surface as S
    S.assign_material = lambda *a, **k: None
    orig_mod = butil.modify_mesh
    def jsonable(v):
        try: json.dumps(v); return v
        except Exception: return repr(v)
    def mm(obj, type, apply=True, **kw):
        i = counter[0]; counter[0] += 1
        tag = f"{i:03d}_{type}"
        if apply and obj.type == "MESH":
            dump_obj(f"{outdir}/{tag}_in.bin", obj)
            json.dump(dict(type=type, kwargs={k: jsonable(v) if k != "texture" else {a: getattr(v, a, None) for a in ["type","noise_scale","noise_basis","noise_depth","noise_type","turbulence","intensity","contrast","stucci_type","use_color_ramp","brightness"]} for k, v in kw.items() if k != "vertex_group"}, vg=kw.get("vertex_group")), open(f"{outdir}/{tag}.json", "w"))
            if kw.get("vertex_group"):
                vg = obj.vertex_groups[kw["vertex_group"]]
                w = np.zeros(len(obj.data.vertices), dtype=np.float32)
                for v in obj.data.vertices:
                    for g in v.groups:
                        if g.group == vg.index: w[v.index] = g.weight
                w.tofile(f"{outdir}/{tag}_vg.f32")
        r = orig_mod(obj, type, apply=apply, **kw)
        if apply and obj.type == "MESH":
            dump_obj(f"{outdir}/{tag}_out.bin", obj)
        return r
    butil.modify_mesh = mm
    orig_geo = S.add_geomod
    def ag(obj, fn, *args, **kw):
        i = counter[0]; counter[0] += 1
        tag = f"{i:03d}_geo_{getattr(fn, '__name__', 'fn')}"
        apply = kw.get("apply", True)
        if apply and obj.type == "MESH":
            dump_obj(f"{outdir}/{tag}_in.bin", obj)
        r = orig_geo(obj, fn, *args, **kw)
        if apply and obj.type == "MESH":
            dump_obj(f"{outdir}/{tag}_out.bin", obj)
        return r
    S.add_geomod = ag
    mod, cls = modcls.rsplit(".", 1)
    C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    np.random.seed(seed)
    f = C(seed)
    o = f.spawn_asset(inst)
    bpy.context.view_layer.update()
    ob = o
    dump_obj(f"{outdir}/final.bin", ob, evaluated=True)
    for c in o.children_recursive:
        if c.type == "MESH": print("child", c.name)
    print("TRACE", counter[0], "calls")
main()

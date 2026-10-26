"""refcache.py: cache python reference asset meshes once, so Rust-side checks (target/fast/refcmp) need no Blender.

usage (run with the infinigen venv python; ONE process, many cases):
  refcache.py module.Class:SEEDS[:INSTS] [...]      e.g. seating.chairs.chair.ChairFactory:0-5  tables.dining_table.TableDiningFactory:0,3,7:0,1
  refcache.py --list cases.txt                       one spec per line
  options: --force (recompute existing)  --dir DIR (default $INDOORGEN_REFCACHE)

Each case -> DIR/<Class>/<fseed>_<inst>.bin :
  b"RCv1" | u32 header_len | header json | f32 verts[nv*3] | u32 face_sizes[nf] | u32 indices[sum(face_sizes)]
header: {class, module, fseed, inst, nv, nf, secs, objects, too_big}
Conventions (match src/bin/refcmp.rs): np.random.seed(fseed) before constructing the factory (global-stream ctor draws),
materials stubbed (weighted_sample -> stub, assign_material no-op), spawn_asset(inst), evaluated meshes of the asset object
and all its children merged in world space, polygon vertex order kept.
"""
import sys, os, json, struct, time, importlib
import numpy as np

class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k):
        import bpy
        return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()

def parse_ints(s):
    out = []
    for part in s.split(","):
        if "-" in part:
            a, b = part.split("-"); out += list(range(int(a), int(b) + 1))
        elif part:
            out.append(int(part))
    return out

def main():
    a = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    force = "--force" in a
    d = os.environ.get("INDOORGEN_REFCACHE") or os.path.join(os.environ.get("INDOORGEN_DATA", os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "local-data")), "refcache")
    if "--dir" in a:
        d = a[a.index("--dir") + 1]
    specs = []
    if "--list" in a:
        specs += [l.strip() for l in open(a[a.index("--list") + 1]) if l.strip() and not l.startswith("#")]
    specs += [x for x in a if ":" in x and not x.startswith("--")]
    import bpy
    import infinigen.core.util.random as R
    _orig_ws = R.weighted_sample
    def _ws(*a, **k):
        # consume the same random draws as the real weighted_sample (the constructor stream continues after it), return a stub class
        if os.environ.get("REFCACHE_STUB") != "1":
            try:
                _orig_ws(*a, **k)
            except Exception:
                pass
        return _D
    R.weighted_sample = _ws
    import infinigen.core.surface as S
    S.assign_material = lambda *a, **k: None
    for spec in specs:
        parts = spec.split(":")
        modcls = parts[0]; seeds = parse_ints(parts[1]); insts = parse_ints(parts[2]) if len(parts) > 2 else [0]
        mod, cls = modcls.rsplit(".", 1)
        try:
            C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
        except Exception as e:
            print("REFCACHE IMPORT FAIL", modcls, repr(e)); continue
        os.makedirs(f"{d}/{cls}", exist_ok=True)
        for s in seeds:
            for inst in insts:
                path = f"{d}/{cls}/{s}_{inst}.bin"
                if os.path.exists(path) and not force:
                    continue
                bpy.ops.wm.read_factory_settings(use_empty=True)
                t0 = time.time()
                try:
                    np.random.seed(s)
                    f = C(s)
                    o = f.spawn_asset(inst)
                    bpy.context.view_layer.update()
                except Exception as e:
                    print("REFCACHE SPAWN FAIL", cls, s, inst, repr(e)); continue
                dg = bpy.context.evaluated_depsgraph_get()
                V = []; sizes = []; idx = []; off = 0; nobj = 0
                for ob in [o] + list(o.children_recursive):
                    if ob.type != "MESH": continue
                    e = ob.evaluated_get(dg); m = e.to_mesh()
                    n = len(m.vertices)
                    if n:
                        co = np.empty(n * 3, dtype=np.float32); m.vertices.foreach_get("co", co)
                        mw = np.array(ob.matrix_world, dtype=np.float64)
                        co = (co.reshape(-1, 3).astype(np.float64) @ mw[:3, :3].T + mw[:3, 3]).astype(np.float32)
                        V.append(co)
                        ls = np.empty(len(m.polygons), dtype=np.uint32); m.polygons.foreach_get("loop_total", ls)
                        lv = np.empty(len(m.loops), dtype=np.uint32); m.loops.foreach_get("vertex_index", lv)
                        sizes.append(ls); idx.append(lv + off); off += n; nobj += 1
                    e.to_mesh_clear()
                V = np.concatenate(V) if V else np.zeros((0, 3), np.float32)
                sizes = np.concatenate(sizes) if sizes else np.zeros(0, np.uint32)
                idx = np.concatenate(idx) if idx else np.zeros(0, np.uint32)
                too_big = len(V) > 600_000
                hdr = dict(cls=cls, module=mod, fseed=s, inst=inst, ws_draws=os.environ.get("REFCACHE_STUB") != "1", nv=int(len(V)), nf=int(len(sizes)), secs=round(time.time() - t0, 3), objects=nobj, too_big=too_big)
                hb = json.dumps(hdr).encode()
                with open(path, "wb") as fh:
                    fh.write(b"RCv1"); fh.write(struct.pack("<I", len(hb))); fh.write(hb)
                    if not too_big:
                        fh.write(V.astype("<f4").tobytes()); fh.write(sizes.astype("<u4").tobytes()); fh.write(idx.astype("<u4").tobytes())
                print("REFCACHE", cls, s, inst, hdr["nv"], hdr["nf"], f"{hdr['secs']}s")

main()

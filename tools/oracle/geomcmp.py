"""geomcmp.py module.Class N seed0 rust.glb : full-surface comparison of Python factory (materials stubbed) vs Rust glb.
Both surfaces are sampled (area-weighted); instances are aligned by bounding-box minimum corner; reports symmetric Chamfer mean / p95 (m) and bbox-origin offset."""
import os, sys, importlib, numpy as np, bpy, mathutils
from mathutils.kdtree import KDTree
class _D:
    """stand-in for material generators: weighted_sample(...) -> _D; _D() -> _D(); _D()() -> a real (empty) Blender material"""
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k):
        import bpy
        return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()
import infinigen.core.util.random as R
R.weighted_sample = lambda *a, **k: _D
import infinigen.core.surface as S
S.assign_material = lambda *a, **k: None
a = sys.argv[sys.argv.index("--")+1:] if "--" in sys.argv else sys.argv[1:]
mod, cls = a[0].rsplit(".", 1); N = int(a[1]); s0 = int(a[2]); glb = a[3]
C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
bpy.ops.wm.read_factory_settings(use_empty=True)
rng = np.random.default_rng(0)

def mesh_data(objs):
    """world-space vertices, triangles, edges of all mesh objects, merged"""
    V = []; T = []; E = []; off = 0
    dg = bpy.context.evaluated_depsgraph_get()
    for o in objs:
        if o.type != "MESH": continue
        e = o.evaluated_get(dg); m = e.to_mesh(); m.calc_loop_triangles()
        mw = np.array(o.matrix_world)
        v = np.array([tuple(x.co) for x in m.vertices]).reshape(-1, 3)
        if len(v) == 0: e.to_mesh_clear(); continue
        v = v @ mw[:3, :3].T + mw[:3, 3]
        t = np.array([tuple(tr.vertices) for tr in m.loop_triangles]).reshape(-1, 3)
        ed = np.array([tuple(x.vertices) for x in m.edges]).reshape(-1, 2)
        V.append(v); T.append(t + off); E.append(ed + off); off += len(v); e.to_mesh_clear()
    if not V: return None
    return np.concatenate(V), np.concatenate(T), np.concatenate(E)

def probe_points(V, E):
    """deterministic: every vertex plus 3 evenly spaced points on every edge (no randomness)"""
    a, b = V[E[:, 0]], V[E[:, 1]]
    pts = [V] + [a + (b - a) * f for f in (0.25, 0.5, 0.75)]
    P = np.concatenate(pts)
    if len(P) > 12000:
        P = P[:: int(np.ceil(len(P) / 12000))]  # deterministic stride, no randomness
    return P

def to_bvh(V, T):
    from mathutils.bvhtree import BVHTree
    return BVHTree.FromPolygons([tuple(v) for v in V], [tuple(int(i) for i in t) for t in T])

def dist_to_surface(P, bvh):
    out = np.empty(len(P))
    for i, p in enumerate(P):
        loc = bvh.find_nearest(mathutils.Vector(p))[0]
        out[i] = (loc - mathutils.Vector(p)).length if loc is not None else 9.0
    return out

# python assets
py = {}
for s in range(s0, s0 + N):
    try:
        np.random.seed(s)  # factories that draw dims before FixedSeed (coffee/side/vase) use global state
        f = C(s); o = f.spawn_asset(0); bpy.context.view_layer.update()
        py[s] = mesh_data([o] + list(o.children_recursive))
        for ob in [o] + list(o.children_recursive): ob.hide_viewport = True
    except Exception as e:
        py[s] = None; print("PYERR", s, repr(e)[:80])
pre = set(bpy.data.objects.keys())
bpy.ops.import_scene.gltf(filepath=glb)
rs = {}
for s in range(s0, s0 + N):
    objs = [o for o in bpy.data.objects if o.name not in pre and o.name.startswith(f"{cls}{s - s0}#")]
    rs[s] = mesh_data(objs) if objs else None
tot = []
for s in range(s0, s0 + N):
    A, B = py[s], rs[s]
    if A is None or B is None: print("GEOMCMP", cls, s, "missing", A is None, B is None); continue
    oa, ob = A[0].min(0), B[0].min(0)
    A = (A[0] - oa, A[1], A[2]); B = (B[0] - ob, B[1], B[2])
    diag = np.linalg.norm(A[0].max(0) - A[0].min(0))
    d1 = dist_to_surface(probe_points(A[0], A[2]), to_bvh(B[0], B[1]))   # python verts/edges -> rust surface
    d2 = dist_to_surface(probe_points(B[0], B[2]), to_bvh(A[0], A[1]))   # rust verts/edges -> python surface
    ch = (d1.mean() + d2.mean()) / 2; hd = max(d1.max(), d2.max()); p95 = max(np.percentile(d1, 95), np.percentile(d2, 95))
    print("GEOMCMP", cls, s, "mean %.4f p95 %.4f max %.3f diag %.2f rel %.1f%% (py->rs %.4f, rs->py %.4f)" % (ch, p95, hd, diag, 100 * ch / diag, d1.mean(), d2.mean()))
    if os.environ.get("WORST"):
        PA, PB = probe_points(A[0], A[2]), probe_points(B[0], B[2])
        for nm, P, d in (("py->rs", PA, d1), ("rs->py", PB, d2)):
            idx = np.argsort(-d)[:5]
            print("WORST", cls, s, nm, ["(%.2f,%.2f,%.2f)=%.3f" % (P[i][0], P[i][1], P[i][2], d[i]) for i in idx])
    tot.append((ch, p95, hd, diag))
if tot:
    t = np.array(tot); print("GEOMSUM", cls, "mean %.4f m  p95 %.4f m  max %.3f m  rel %.1f%%" % (t[:, 0].mean(), t[:, 1].mean(), t[:, 2].mean(), 100 * (t[:, 0] / t[:, 3]).mean()))

"""clothcmp.py SCENE.glb REF.cloth.glb TEST.cloth.glb : compare two cloth overlays of the same scene (tools/cloth_only.py outputs, e.g. full
colliders vs CLOTH_COL_MAX). Per simulated node: per-vertex drape difference (same topology) and, for cloth that collides with other
nodes, how deep its vertices sit inside the REFERENCE (full-detail) colliders -- penetration that a simplified collider could add.
Run with the Blender-5.2 venv (needs mathutils): $BLENDER_PY tools/clothcmp.py SCENE.glb REF.cloth.glb TEST.cloth.glb"""
import os, sys, json
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cloth_only import Glb
import bpy  # noqa: F401 (makes mathutils importable)
from mathutils import Vector
from mathutils.bvhtree import BVHTree


def depth_inside(v, cv, cf):
    """per vertex of `v`: how far it is inside the surface (cv, cf) along the nearest face normal (0 when outside)"""
    t = BVHTree.FromPolygons([tuple(p) for p in cv], [tuple(int(i) for i in f) for f in cf])
    d = np.zeros(len(v))
    for i, p in enumerate(v):
        hit = t.find_nearest(Vector(p), 0.05)
        if hit[0] is None:
            continue
        s = (Vector(p) - hit[0]).dot(hit[1])
        if s < 0:
            d[i] = -s
    return d


def main():
    scene, ref, test = sys.argv[1:4]
    g, r, t = Glb(scene), Glb(ref), Glb(test)
    jobs = [j for j in json.load(open(scene.replace(".glb", ".json")))["cloth"] if j["kind"] == "cloth"]
    mesh_ref = lambda n: r.mesh(n) if n in r.by_name else g.mesh(n)
    for j in jobs:
        n = j["target"]
        if n not in r.by_name or n not in t.by_name:
            continue
        vr, _ = r.mesh(n)
        vt, _ = t.mesh(n)
        line = f"{n:28s}"
        if len(vr) == len(vt):
            d = np.linalg.norm(vr - vt, axis=1) * 1000
            line += f" drape diff mean {d.mean():6.2f} mm  p95 {np.percentile(d, 95):6.2f}  max {d.max():6.2f}"
        else:
            line += f" topology differs ({len(vr)} vs {len(vt)} verts)"
        if j["collide"]:
            pr = np.zeros(len(vr)); pt = np.zeros(len(vt))
            for c in j["collide"]:
                cv, cf = mesh_ref(c)
                pr = np.maximum(pr, depth_inside(vr, cv, cf))
                pt = np.maximum(pt, depth_inside(vt, cv, cf))
            f = lambda p: f"{100 * (p > 0.001).mean():5.2f}% >1mm, {100 * (p > 0.003).mean():5.2f}% >3mm, max {p.max() * 1000:5.1f} mm"
            line += f" | inside full colliders: ref {f(pr)} | test {f(pt)}"
        print(line)


main()

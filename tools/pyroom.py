"""Stage dump of the python room pipeline (polygon2obj -> weld -> solidify -> solidify -> triangulate) for every room of a
fixture.  usage: python tools/pyroom.py case_state0.json out.json [wall_thickness wall_height]
(polygon = bottom ring of the fixture room; wall thickness/height default to the values inferred from the fixture)"""
import sys, json, numpy as np, bpy
from shapely.geometry import Polygon
from infinigen.assets.utils.shapes import polygon2obj
from infinigen.core.util import blender as butil
d = json.load(open(sys.argv[1]))
res = {}
for name, o in d["objs"].items():
    if name in ("door", "entrance") or name.startswith(("door", "window", "entrance")) or "/" not in name:
        continue
    V = np.array(o["verts_world"]); n = len(V) // 4
    ring = V[:n, :2]
    H = float(V[:, 2].max())
    if len(sys.argv) > 4:
        wt, H = float(sys.argv[3]), float(sys.argv[4])
    else:
        wt = 2 * float(V[:, 2][V[:, 2] > 1e-6].min())
    out = {}
    def snap(ob, k):
        me = ob.data
        out[k] = {"v": [list(map(float, v.co)) for v in me.vertices], "p": [list(p.vertices) for p in me.polygons], "e": [list(e.vertices) for e in me.edges]}
    obj = polygon2obj(Polygon(ring), True, dissolve=False)
    snap(obj, "fill")
    butil.modify_mesh(obj, "WELD", merge_threshold=0.01); snap(obj, "weld")
    butil.modify_mesh(obj, "SOLIDIFY", thickness=H, offset=-1); snap(obj, "sol1")
    obj.vertex_groups.new(name="visible_")
    butil.modify_mesh(obj, "SOLIDIFY", thickness=wt / 2, offset=-1, use_even_offset=True, shell_vertex_group="visible_", use_quality_normals=True)
    snap(obj, "sol2")
    with butil.ViewportMode(obj, "EDIT"):
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.mesh.quads_convert_to_tris(quad_method="BEAUTY", ngon_method="BEAUTY")
    snap(obj, "tri")
    res[name] = out
json.dump(res, open(sys.argv[2], "w"))

"""Python room pipeline (polygon2obj -> WELD -> SOLIDIFY -> SOLIDIFY -> quads_convert_to_tris) on rings dumped by
`roomcmp dumpin`.  usage: python tools/pyroom2.py in.json out.json  ->  {seed: {room: {v:[...], p:[...]}}}"""
import sys, json, numpy as np, bpy
from shapely.geometry import Polygon
from infinigen.assets.utils.shapes import polygon2obj
from infinigen.core.util import blender as butil
d = json.load(open(sys.argv[1]))
res = {}
for seed, sd in d.items():
    wt, H = sd["wt"], sd["h"]
    res[seed] = {}
    for name, rings in sd["rooms"].items():
        p = Polygon(rings[0], rings[1:])
        obj = polygon2obj(p, True, dissolve=False)
        fill = {"v": [list(map(float, v.co)) for v in obj.data.vertices], "p": [list(q.vertices) for q in obj.data.polygons]}
        butil.modify_mesh(obj, "WELD", merge_threshold=0.01)
        butil.modify_mesh(obj, "SOLIDIFY", thickness=H, offset=-1)
        me1 = obj.data
        dbg = {"fn": [list(map(float, q.normal)) for q in me1.polygons], "vn": [list(map(float, v.normal)) for v in me1.vertices], "v": [list(map(float, v.co)) for v in me1.vertices], "p": [list(q.vertices) for q in me1.polygons]}
        obj.vertex_groups.new(name="visible_")
        butil.modify_mesh(obj, "SOLIDIFY", thickness=wt / 2, offset=-1, use_even_offset=True, shell_vertex_group="visible_", use_quality_normals=True)
        with butil.ViewportMode(obj, "EDIT"):
            bpy.ops.mesh.select_all(action="SELECT")
            bpy.ops.mesh.quads_convert_to_tris(quad_method="BEAUTY", ngon_method="BEAUTY")
        me = obj.data
        res[seed][name] = {"fill": fill, "dbg": dbg, "v": [list(map(float, v.co)) for v in me.vertices], "p": [list(q.vertices) for q in me.polygons]}
        bpy.data.objects.remove(obj)
json.dump(res, open(sys.argv[2], "w"))

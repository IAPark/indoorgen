"""pycoil.py out_prefix w d t dd ds dp ru sf : python make_coiled stages on a subdivided box -> out_prefix.<stage>.json {verts,polys}
stages: grid poke1 quads poke3 scale tri final  (compare with: STAGE=.. RU=.. SF=.. target/fast/coiltest out.json w d t dd ds dp)"""
import sys, json, numpy as np, bpy, bmesh
from infinigen.assets.utils.decorate import read_co, subdivide_edge_ring
from infinigen.assets.utils.object import new_cube
from infinigen.core import surface
from infinigen.core.nodes import Nodes, NodeWrangler
from infinigen.core.surface import write_attr_data
from infinigen.core.util import blender as butil
a = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
pre = a[0]; w, d, t, dd, ds, dp, ru, sf = map(float, a[1:9])
def dump(obj, name):
    me = obj.data
    V = read_co(obj).tolist(); P = [list(p.vertices) for p in me.polygons]
    json.dump({"verts": V, "polys": P}, open(f"{pre}.{name}.json", "w"))
bpy.ops.wm.read_factory_settings(use_empty=True)
obj = new_cube(); obj.scale = w / 2, d / 2, t / 2; butil.apply_transform(obj)
for i, size in enumerate(obj.dimensions):
    axis = np.zeros(3); axis[i] = 1
    subdivide_edge_ring(obj, int(np.ceil(size / dd)), axis)
dump(obj, "grid")
with butil.ViewportMode(obj, "EDIT"):
    bpy.ops.mesh.select_mode(type="FACE"); bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.mesh.poke()
obj.data.update(); dump(obj, "poke1")
with butil.ViewportMode(obj, "EDIT"):
    bpy.ops.mesh.tris_convert_to_quads()
dump(obj, "quads")
with butil.ViewportMode(obj, "EDIT"):
    bpy.ops.mesh.poke(); bpy.ops.mesh.poke()
    bpy.ops.mesh.select_all(action="DESELECT")
    bm = bmesh.from_edit_mesh(obj.data)
    for v in bm.verts:
        if len(v.link_edges) == 16: v.select_set(True)
    bm.select_flush(False); bmesh.update_edit_mesh(obj.data)
    radius = dd * ru
    bpy.ops.mesh.bevel(offset=radius, affect="VERTICES")
dump(obj, "bevel")
with butil.ViewportMode(obj, "EDIT"):
    bpy.ops.mesh.extrude_region_shrink_fatten(TRANSFORM_OT_shrink_fatten={"value": -dp})
    bpy.ops.mesh.extrude_region_shrink_fatten(TRANSFORM_OT_shrink_fatten={"value": dp})
    bpy.ops.mesh.select_more(); bpy.ops.mesh.select_more()
dump(obj, "extr")
write_attr_data(obj, "tip", np.zeros(len(obj.data.polygons)), domain="FACE")
with butil.ViewportMode(obj, "EDIT"):
    surface.set_active(obj, "tip"); bpy.ops.mesh.attribute_set(value_float=1)
def geo_scale(nw):
    g = nw.new_node(Nodes.GroupInput, expose_input=[("NodeSocketGeometry", "Geometry", None)])
    sel = nw.new_node(Nodes.NamedAttribute, ["tip"])
    g = nw.new_node(Nodes.ScaleElements, [g, sel, nw.combine(*([ds / radius] * 3))])
    nw.new_node(Nodes.GroupOutput, input_kwargs={"Geometry": g})
surface.add_geomod(obj, geo_scale, apply=True)
dump(obj, "scale")
butil.modify_mesh(obj, "TRIANGULATE", min_vertices=4)
dump(obj, "tri")
butil.modify_mesh(obj, "SMOOTH", factor=sf, iterations=5)
dump(obj, "final")

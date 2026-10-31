"""tree_cache.py KIND VARIANT OUT.blend: build one Infinigen TreeFactory / BushFactory variant (factory seed VARIANT, instance 0) and save it
as a collection "outdoor_<kind>_<variant>" in OUT.blend, for render_rust.py to instance outside the windows (sidecar `outdoor.props`).
Runs under Blender 4.2 (infinigen's tree code predates Blender 5: removed shader nodes, typed sockets); the saved file opens in 5.x.
The objects keep their geometry-nodes modifiers (Blender 5 versions and evaluates them); the trunk base sits at the origin."""
import sys
import bpy
import mathutils

kind, variant, out = sys.argv[-3], int(sys.argv[-2]), sys.argv[-1]
from infinigen.assets.objects.trees.generate import BushFactory, TreeFactory

bpy.ops.wm.read_factory_settings(use_empty=True)
fac = (TreeFactory if kind == "tree" else BushFactory)(variant, coarse=False)
# detail as seen from a window (full detail made a 12.8 M-polygon bark mesh)
import os
# (face_size directly: target_face_size needs a scene camera and falls back to its 3 mm floor without one)
root = fac.spawn_asset(0, face_size=float(os.environ.get("TREE_FACE", 0.03 if kind == "tree" else 0.01)))
objs = [o for o in [root] + list(root.children_recursive)]
bpy.context.view_layer.update()
# keep the generated objects as they are (leaves are geometry-nodes instances: realizing them made a 137 M-polygon tree); Blender 5
# versions the node groups on load and evaluates them. Base at the origin.
coll = bpy.data.collections.new(f"outdoor_{kind}_{variant}")
bpy.context.scene.collection.children.link(coll)
for o in objs:
    for c in list(o.users_collection):
        c.objects.unlink(o)
    coll.objects.link(o)
dg = bpy.context.evaluated_depsgraph_get()
zmin = min((inst.matrix_world @ mathutils.Vector(c)).z for inst in dg.object_instances if inst.object.original in objs or (inst.parent and inst.parent.original in objs) for c in inst.object.bound_box)
root.location.z -= zmin
root.name = f"outdoor_{kind}_{variant}"
npoly = sum(len(o.data.polygons) for o in objs if o.type == "MESH")
print("TREECACHE", kind, variant, len(objs), "objects", npoly, "base polys", [round(x, 2) for x in root.dimensions], "zmin", round(zmin, 3))
bpy.data.libraries.write(out, {coll}, fake_user=True, compress=True)

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
import os
# leafy seasons only (TreeFactory draws winter 1 in 4: bare trees outside every window); TREE_SEASON overrides
_season = os.environ.get("TREE_SEASON") or ["summer", "summer", "spring", "autumn"][variant % 4]
fac = TreeFactory(variant, season=_season, coarse=False) if kind == "tree" else BushFactory(variant, coarse=False)
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
ninst = sum(1 for i in dg.object_instances if i.is_instance)
print("TREECACHE", kind, variant, _season, "instances", ninst, len(objs), "objects", npoly, "base polys", [round(x, 2) for x in root.dimensions], "zmin", round(zmin, 3))
# bake: every evaluated piece (bark, twigs, leaves, fruit -- the leaves are geometry-nodes instances whose node groups evaluate to
# nothing in Blender 5) into one plain mesh with its materials; BAKE=0 keeps the live objects
if os.environ.get("BAKE", "1") != "0":
    dg = bpy.context.evaluated_depsgraph_get()
    # one plain mesh per unique piece (each twig / leaf / fruit kind, with its own evaluated geometry), shared by every copy of it:
    # linked duplicates (realizing every copy made a 60 M-polygon, 2 GB tree); fruit dropped (unripe-looking blobs from a window)
    protos, placed = {}, []
    for inst in dg.object_instances:
        ob = inst.object
        top = inst.parent.original if inst.is_instance and inst.parent else ob.original
        if ob.type != "MESH" or top not in objs or "Fruit" in ob.original.name: continue
        key = ob.original.name
        if key not in protos:
            me = bpy.data.meshes.new_from_object(ob, preserve_all_data_layers=True, depsgraph=dg)
            protos[key] = me if len(me.polygons) else None
        if protos[key] is not None: placed.append((protos[key], inst.matrix_world.copy()))
    for o in objs: bpy.data.objects.remove(o, do_unlink=True)
    rootob = bpy.data.objects.new(f"outdoor_{kind}_{variant}", None); coll.objects.link(rootob)
    for me, m in placed:
        nob = bpy.data.objects.new("leafpart", me); nob.matrix_world = m; nob.parent = rootob; coll.objects.link(nob)
    npoly = sum(len(me.polygons) for me, _ in placed)
    print("TREEBAKE", kind, variant, len(protos), "unique pieces", len(placed), "copies", npoly, "polys rendered", sum(len(m.polygons) for m in protos.values() if m), "polys stored")
bpy.data.libraries.write(out, {coll}, fake_user=True, compress=True)

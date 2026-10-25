"""lidparts.py seed out.json : LidFactory create_asset parts (before join) as verts/polys"""
import sys, json, numpy as np, bpy
from infinigen.assets.objects.tableware.lid import LidFactory
from infinigen.core.util import blender as butil
seed=int(sys.argv[1]); outp=sys.argv[2]
bpy.ops.wm.read_factory_settings(use_empty=True)
parts={}
import infinigen.assets.objects.tableware.lid as L
orig=L.join_objects
def jo(objs, *a, **k):
    for i,o in enumerate(objs):
        me=o.data
        parts[f"part{i}"]={"v":[list(o.matrix_world@v.co) for v in me.vertices],"p":[list(p.vertices) for p in me.polygons]}
    return orig(objs,*a,**k)
L.join_objects=jo
f=LidFactory(seed)
parts["params"]={k:(getattr(f,k) if isinstance(getattr(f,k),(str,bool)) else float(getattr(f,k))) for k in ["x_length","z_height","thickness","is_glass","rim_height","handle_type","handle_height","handle_radius","handle_width","handle_subsurf_level"]}
o=f.spawn_asset(0)
me=o.data
parts['final']={'v':[list(o.matrix_world@v.co) for v in me.vertices],'p':[list(p.vertices) for p in me.polygons]}
json.dump(parts,open(outp,"w"))
print(parts["params"],{k:len(v["v"]) for k,v in parts.items() if k!="params"})

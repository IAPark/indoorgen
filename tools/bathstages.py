"""bathstages.py seed inst out.json : Bathtub (freestanding) intermediate meshes"""
import sys, json, numpy as np, bpy, bmesh
from numpy.random import uniform
from infinigen.assets.objects.bathroom.bathtub import BathtubFactory
from infinigen.assets.utils.decorate import subsurf, read_co
from infinigen.assets.utils.object import join_objects
from infinigen.core.util import blender as butil
seed=int(sys.argv[1]); inst=int(sys.argv[2]); outp=sys.argv[3]
bpy.ops.wm.read_factory_settings(use_empty=True)
stages={}
def snap(name, obj):
    dg=bpy.context.evaluated_depsgraph_get(); e=obj.evaluated_get(dg); me=e.to_mesh()
    stages[name]={"v":[list(v.co) for v in me.vertices],"p":[list(p.vertices) for p in me.polygons],"e":[list(x.vertices) for x in me.edges]}
    e.to_mesh_clear()
class B(BathtubFactory):
    def make_bowl(self):
        o=super().make_bowl(); snap("bowl",o); return o
    def remove_top(self,obj):
        super().remove_top(obj); snap("removetop",obj)
    def make_legs(self,obj):
        snap("prelegs",obj)
        l=super().make_legs(obj)
        for i,x in enumerate(l): snap(f"leg{i}",x)
        return l
    def add_base(self,obj):
        o=super().add_base(obj); snap("base",o); return o
    def add_hole(self,obj):
        snap("prehole",obj)
        h=super().add_hole(obj); snap("hole",h); return h
    def make_freestanding(self):
        o=super().make_freestanding(); snap("freestanding",o); return o
np.random.seed(seed)
f=B(seed)
stages["params"]={k:(float(getattr(f,k)) if isinstance(getattr(f,k),(int,float,np.floating,np.integer)) else str(getattr(f,k))) for k in ["width","size","depth","thickness","leg_height","leg_side","leg_radius","leg_y_scale","leg_subsurf_level","taper_factor","stretch_factor","alcove_levels","side_levels","levels","hole_radius","has_legs","has_curve","bathtub_type","has_base"]}
stages["disp_x"]=list(map(float,f.disp_x)); stages["disp_y"]=float(f.disp_y)
o=f.spawn_asset(inst)
snap("final", o)
json.dump(stages, open(outp,"w"))
print(stages["params"]["bathtub_type"], {k:len(v["v"]) for k,v in stages.items() if isinstance(v,dict) and "v" in v})

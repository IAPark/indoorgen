"""tvstages.py seed inst out.json : TVFactory intermediate meshes (make_base result, after make_screen)"""
import sys, json, numpy as np, bpy, bmesh
import infinigen.core.util.random as R
class _D:
    def __init__(self,*a,**k): pass
    def __call__(self,*a,**k): return bpy.data.materials.new("stub")
    def __getattr__(self,n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()
_o=R.weighted_sample
R.weighted_sample=lambda *a,**k:(_o(*a,**k),_D)[1]
import infinigen.core.surface as S
S.assign_material=lambda *a,**k:None
from infinigen.assets.objects.appliances.tv import TVFactory, MonitorFactory
seed=int(sys.argv[1]); inst=int(sys.argv[2]); outp=sys.argv[3]
bpy.ops.wm.read_factory_settings(use_empty=True)
st={}
def snap(name,obj):
    dg=bpy.context.evaluated_depsgraph_get(); e=obj.evaluated_get(dg); me=e.to_mesh()
    st[name]={"v":[list(v.co) for v in me.vertices],"p":[list(p.vertices) for p in me.polygons]}
    e.to_mesh_clear()
class T(TVFactory):
    def make_base(self):
        o=super().make_base(); snap("base",o); return o
    def make_screen(self,obj):
        super().make_screen(obj); snap("screen",obj)
np.random.seed(seed)
f=T(seed)
st["params"]={k:(float(getattr(f,k)) if isinstance(getattr(f,k),(int,float,np.floating,np.integer,bool,np.bool_)) else str(getattr(f,k))) for k in ["width","height","depth","side_margin","bottom_margin","depth_extrude","has_depth_extrude","screen_bevel_width","leg_type"]}
o=f.spawn_asset(inst); snap("final",o)
json.dump(st,open(outp,"w")); print(st["params"]["has_depth_extrude"], {k:len(v["v"]) for k,v in st.items() if k!="params"})

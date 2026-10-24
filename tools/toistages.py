"""toistages.py seed inst out.json : Toilet intermediate meshes (python) + params. materials stubbed (like geomcmp.py)."""
import sys, json, numpy as np, bpy
class _D:
    def __init__(self,*a,**k): pass
    def __call__(self,*a,**k): return bpy.data.materials.new("stub")
    def __getattr__(self,n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()
import infinigen.core.util.random as R
R.weighted_sample=lambda *a,**k:_D
import infinigen.core.surface as S
S.assign_material=lambda *a,**k:None
from infinigen.assets.objects.bathroom.toilet import ToiletFactory
seed=int(sys.argv[-3]); inst=int(sys.argv[-2]); outp=sys.argv[-1]
bpy.ops.wm.read_factory_settings(use_empty=True)
stages={}
def snap(name,obj):
    if obj.type!='MESH': return
    me=obj.data
    mw=np.array(obj.matrix_world)
    stages[name]={"v":[list(map(float,mw[:3,:3]@np.array(v.co)+mw[:3,3])) for v in me.vertices],"p":[list(p.vertices) for p in me.polygons],"e":[list(x.vertices) for x in me.edges]}
import infinigen.core.util.blender as butil
_cnt=[0]
_orig_mm=butil.modify_mesh
def mm(obj,type,*a,**k):
    r=_orig_mm(obj,type,*a,**k)
    if type!='MIRROR' or True:
        _cnt[0]+=1; snap(f"m{_cnt[0]:02d}_{type}",obj)
    return r
butil.modify_mesh=mm
_orig_exit=butil.ViewportMode.__exit__
def ex(self,*a):
    r=_orig_exit(self,*a)
    _cnt[0]+=1; snap(f"e{_cnt[0]:02d}_edit",self.obj if hasattr(self,'obj') else bpy.context.active_object)
    return r
butil.ViewportMode.__exit__=ex
class T(ToiletFactory):
    def build_curve(self):
        o=super().build_curve(); snap("curve",o); return o
    def make_tube(self,lower,upper):
        snap("upper",upper); snap("lower",lower)
        o=super().make_tube(lower,upper); snap("tube",o); return o
    def make_seat(self,obj):
        s,c=super().make_seat(obj); snap("seat",s); snap("cover",c); return s,c
    def make_plane(self,obj):
        o=super().make_plane(obj); snap("plane",o); return o
    def make_stand(self,obj,bottom):
        snap("bottom",bottom); snap("tube_pre_stand",obj)
        o=super().make_stand(obj,bottom); snap("stand",o); return o
    def make_back(self,obj):
        o=super().make_back(obj); snap("back",o); return o
    def make_tank(self):
        o=super().make_tank(); snap("tank",o); return o
    def add_button(self):
        o=super().add_button(); snap("hw",o); return o
    def add_handle(self):
        o=super().add_handle(); snap("hw",o); return o
np.random.seed(seed)
f=T(seed)
keys=["size","width","height","size_mid","curve_scale","depth","tube_scale","thickness","extrude_height","stand_depth","stand_scale","bottom_offset","back_thickness","back_size","back_scale","seat_thickness","seat_size","has_seat_cut","tank_width","tank_height","tank_size","tank_cap_height","tank_cap_extrude","cover_rotation","hardware_type","hardware_cap","hardware_radius","hardware_length","hardware_on_side"]
stages["params"]={k:(float(getattr(f,k)) if isinstance(getattr(f,k),(int,float,np.floating,np.integer,bool,np.bool_)) else str(getattr(f,k))) for k in keys}
o=f.spawn_asset(inst); snap("final",o)
json.dump(stages,open(outp,"w"))
print({k:(len(v["v"]),len(v["p"])) for k,v in stages.items() if k!="params"})

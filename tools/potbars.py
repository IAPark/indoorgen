"""potbars.py seed out.json : PotFactory.add_bar intermediates per side: torus (pre-boolean, local), boolean result pieces, selected bar after subsurf (local coords)"""
import sys, json, numpy as np, bpy
from numpy.random import uniform
from infinigen.assets.objects.tableware.pot import PotFactory
from infinigen.assets.utils.decorate import read_co, subsurf, write_attribute
from infinigen.assets.utils.object import join_objects
from infinigen.core.util import blender as butil
seed=int(sys.argv[1]); outp=sys.argv[2]
bpy.ops.wm.read_factory_settings(use_empty=True)
st={}
def snap(name,obj):
    dg=bpy.context.evaluated_depsgraph_get(); e=obj.evaluated_get(dg); me=e.to_mesh()
    st[name]={"v":[list(v.co) for v in me.vertices],"p":[list(p.vertices) for p in me.polygons],"loc":list(obj.location)}
    e.to_mesh_clear()
class P(PotFactory):
    def add_bar(self, obj):
        bars=[]
        for side in [-1,1]:
            bpy.ops.mesh.primitive_torus_add(location=(side*(1+self.bar_x_offset),0,self.bar_height),major_radius=self.bar_radius,minor_radius=self.bar_inner_radius)
            bar=bpy.context.active_object
            bar.scale=self.bar_scale
            butil.modify_mesh(bar,"SIMPLE_DEFORM",deform_method="TAPER",angle=self.bar_taper,deform_axis="X")
            bar.rotation_euler=0,self.bar_y_rotation,0 if side==1 else np.pi
            butil.apply_transform(bar)
            snap(f"torus{side}",bar)
            butil.modify_mesh(bar,"BOOLEAN",object=obj,operation="DIFFERENCE")
            snap(f"bool{side}",bar)
            butil.select_none()
            objs=butil.split_object(bar)
            i=np.argmax([np.max(read_co(o)[:,0]*side) for o in objs])
            bar=objs[i]; objs.remove(bar); butil.delete(objs)
            snap(f"piece{side}",bar)
            subsurf(bar,1)
            snap(f"bar{side}",bar)
            write_attribute(bar,lambda nw:1,"guard","FACE")
            bars.append(bar)
        return join_objects([obj,*bars])
np.random.seed(seed)
f=P(seed)
st["params"]={k:(float(getattr(f,k))) for k in ["bar_height","bar_radius","bar_x_offset","bar_taper","bar_y_rotation","depth","thickness"]}
st["params"]["bar_scale"]=list(map(float,f.bar_scale))
o=f.spawn_asset(0)
json.dump(st,open(outp,"w"))
print({k:(len(v["v"]) if isinstance(v,dict) and "v" in v else v) for k,v in st.items()})

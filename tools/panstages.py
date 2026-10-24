"""panstages.py seed inst out.json : dump Pan make_base intermediate meshes (verts, polys)"""
import sys, json, numpy as np, bpy, bmesh
from numpy.random import uniform
from infinigen.assets.objects.tableware.pan import PanFactory
from infinigen.assets.utils.decorate import subsurf, read_co
from infinigen.assets.utils.object import join_objects, new_base_circle, new_base_cylinder, origin2lowest
from infinigen.core.util import blender as butil
from infinigen.core.util.random import log_uniform
from infinigen.core.util.math import FixedSeed, int_hash
seed=int(sys.argv[1]); inst=int(sys.argv[2]); outp=sys.argv[3]
bpy.ops.wm.read_factory_settings(use_empty=True)
stages={}
def snap(name, obj):
    dg=bpy.context.evaluated_depsgraph_get(); e=obj.evaluated_get(dg); me=e.to_mesh()
    stages[name]={"v":[list(v.co) for v in me.vertices],"p":[list(p.vertices) for p in me.polygons],"e":[list(x.vertices) for x in me.edges]}
    e.to_mesh_clear()
class P(PanFactory):
    def make_base(self):
        n = 4 * int(log_uniform(4, 8))
        stages["n"]=n
        base = new_base_circle(vertices=n)
        middle = new_base_circle(vertices=n)
        middle.location[-1] = self.depth / 2
        middle.scale = [self.r_mid] * 3
        upper = new_base_circle(vertices=n)
        upper.location[-1] = self.depth
        upper.scale = [self.r_expand] * 3
        butil.apply_transform(upper, loc=True)
        obj = join_objects([base, middle, upper])
        snap("joined", obj)
        with butil.ViewportMode(obj, "EDIT"):
            bpy.ops.mesh.bridge_edge_loops()
            bm = bmesh.from_edit_mesh(obj.data)
            for v in bm.verts:
                v.select_set(np.abs(v.co[-1]) < 1e-3)
            bm.select_flush(False)
            bmesh.update_edit_mesh(obj.data)
        snap("bridged", obj)
        off = np.random.randint(n // 4)
        stages["fill_offset"]=int(off)
        with butil.ViewportMode(obj, "EDIT"):
            bpy.ops.mesh.fill_grid(use_interp_simple=True, offset=off)
            bpy.ops.mesh.quads_convert_to_tris(quad_method="BEAUTY", ngon_method="BEAUTY")
        snap("filled", obj)
        obj.rotation_euler[-1] = np.pi / n
        butil.apply_transform(obj)
        if self.has_handle:
            self.add_handle(obj)
        snap("handle", obj)
        self.solidify_with_inside(obj, self.thickness)
        snap("solid", obj)
        def selection(nw, x):
            return nw.compare("GREATER_THAN", x, self.x_guard)
        self.add_guard(obj, selection)
        snap("guard", obj)
        subsurf(obj, 1, True)
        snap("simple1", obj)
        subsurf(obj, 3)
        snap("cc3", obj)
        if self.has_handle_hole:
            self.add_handle_hole(obj)
        snap("hole", obj)
        return obj
np.random.seed(seed)
f=P(seed)
stages["params"]={k:(float(getattr(f,k)) if isinstance(getattr(f,k),(int,float,np.floating,np.integer)) else str(getattr(f,k))) for k in ["r_expand","depth","r_mid","has_handle_hole","x_handle","z_handle","z_handle_mid","s_handle","thickness","has_guard","x_guard","guard_depth","scale"]}
o=f.spawn_asset(inst)
snap("final", o)
json.dump(stages, open(outp,"w"))
print({k:(len(v["v"]) if isinstance(v,dict) and "v" in v else v) for k,v in stages.items() if k!="params"})

"""pyapply.py : for each material class, seed s: inst=C(); variant apply; print next uniform (to validate rust apply-draw emulation)"""
import sys, json, numpy as np, bpy
from infinigen.assets.composition import material_assignments as MA
from infinigen.assets.materials import metal, wood, plastic, ceramic
bpy.ops.wm.read_factory_settings(use_empty=True)
classes = {"metal.MetalBasic": metal.MetalBasic, "metal.BrushedMetal": metal.BrushedMetal, "metal.GalvanizedMetal": metal.GalvanizedMetal, "metal.GrainedMetal": metal.GrainedMetal, "metal.HammeredMetal": metal.HammeredMetal, "metal.BrushedBlackMetal": metal.BrushedBlackMetal,
 "wood.Wood": wood.Wood, "wood.WhitePlywood": wood.WhitePlywood, "wood.BlackPlywood": wood.BlackPlywood, "wood.BlondePlywood": wood.BlondePlywood,
 "plastic.Plastic": plastic.Plastic, "plastic.PlasticRough": plastic.PlasticRough, "plastic.PlasticTranslucent": plastic.PlasticTranslucent, "ceramic.Glass": ceramic.Glass}
bpy.ops.mesh.primitive_plane_add(); ob = bpy.context.active_object
out = {}
for name, C in classes.items():
    for s in range(1, 7):
        for var in ("empty_mc", "obj", "glass"):
            np.random.seed(s); inst = C(); p_init = float(np.random.uniform()); np.random.seed(s); inst = C()
            if var == "empty_mc": inst.apply([], metal_color=(0.1, 0.5, 0.5))
            elif var == "obj": inst.apply(ob)
            else:
                try: inst.apply(ob, selection="glass", clear=True)
                except Exception as e: out["%s/%d/%s" % (name, s, var)] = str(e)[:40]; continue
            out["%s/%d/%s" % (name, s, var)] = [p_init, float(np.random.uniform())]
json.dump(out, open(sys.argv[1], "w")); print("PYAPPLY", len(out))

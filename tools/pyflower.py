"""export the MonocotGrowthFactory.make_geo_flower node group (tiny leaf collection) to data/gn/monocot_flower.json"""
import sys, json, numpy as np, bpy
sys.argv = [sys.argv[0]]
sys.path.insert(0, "tools")
import gn_export as G
from infinigen.assets.objects.monocot.agave import AgaveMonocotFactory
from infinigen.core import surface
from infinigen.assets.utils.object import data2mesh, mesh2obj
from infinigen.core.placement.factory import make_asset_collection
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(0)
f = AgaveMonocotFactory(0)
def mk(i, **k):
    return mesh2obj(data2mesh([[0,0,0],[1,0,0],[1,1,0]], [], [[0,1,2]]))
leaves = make_asset_collection(mk, 3, "leaves", verbose=False)
obj = mesh2obj(data2mesh([[0,0,0],[0,0,1]], [[0,1]]))
surface.add_geomod(obj, f.make_geo_flower(), apply=False, input_args=[leaves])
mod = obj.modifiers[-1]
out = G.new_out()
G.export_group(mod.node_group, out)
out["main"] = mod.node_group.name
json.dump(out, open("data/gn/monocot_flower.json", "w"), separators=(",", ":"))
print("exported", mod.node_group.name, len(out["groups"][mod.node_group.name]["nodes"]))

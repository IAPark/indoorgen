"""drawlog.py MODULE.CLASS SEED INST OUT.json : log every numpy/random draw (and np.random.seed calls) during C(seed) and spawn_asset(INST)"""
import sys, json, importlib, random as pyrandom, numpy as np
sys.path.insert(0, __import__("os").path.dirname(__import__("os").path.abspath(__file__)))
import bpy
EV = []
PH = ["ctor"]
def num(x):
    if isinstance(x, (np.floating, float)): return float(x)
    if isinstance(x, (np.integer, int)): return int(x)
    if isinstance(x, np.ndarray): return [num(y) for y in x.ravel().tolist()]
    if isinstance(x, (list, tuple)): return [num(y) for y in x]
    return str(x)
def wrap(name):
    f = getattr(np.random, name)
    def g(*a, **k):
        r = f(*a, **k)
        EV.append({"ph": PH[0], "f": name, "a": num(list(a)), "k": {kk: num(v) for kk, v in k.items() if kk in ("size", "p")}, "r": num(r)})
        return r
    setattr(np.random, name, g)
for n in ("uniform", "normal", "randint", "choice", "rand", "random", "random_sample", "shuffle", "standard_normal", "permutation", "randn", "seed"):
    wrap(n)
a = sys.argv[sys.argv.index("--")+1:] if "--" in sys.argv else sys.argv[1:]
mod, cls = a[0].rsplit(".", 1); seed = int(a[1]); inst = int(a[2]); out = a[3]
import gn_export as GX
GX.stub_materials()
C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(seed)
f = C(seed)
PH[0] = "asset"
o = f.spawn_asset(inst)
json.dump(EV, open(out, "w"))
print("events", len(EV), "ctor", sum(1 for e in EV if e["ph"] == "ctor"))

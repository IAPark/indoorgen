"""clothcheck_bed.py SEED py_prefix [map]: Rust-exported pre-sim bed cloth + Blender's cloth solver vs the python bed.
  1. target/fast/refcmp BedFactory SEED --raw --dump D --cloth-json J          (Rust parts + jobs, asset-local = world)
  2. run the jobs (tools/cloth_jobs.py) on those parts
  3. compare every job target with the python object dumped by tools/pybedfull.py (py_prefix.<k>.json)
map: python dict {rust target name -> python object index}, e.g. "{'bed#4': 2, 'bed#5': 1, 'bed#3': 3}" (default: sheet 4->2, cover 5->1, mattress 3->3)"""
import sys, os, json, subprocess, numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cloth_jobs
from scipy.spatial import cKDTree
seed = int(sys.argv[1]); pyp = sys.argv[2]
mp = eval(sys.argv[3]) if len(sys.argv) > 3 else {"bed#4": 2, "bed#5": 1, "bed#3": 3}
root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
tmp = os.path.join(root, "tmp", f"cc{seed}"); os.makedirs(tmp, exist_ok=True)
subprocess.run([f"{root}/target/fast/refcmp", "BedFactory", str(seed), "--raw", "--dump", tmp, "--cloth-json", f"{tmp}/jobs.json"], capture_output=True)
jobs = json.load(open(f"{tmp}/jobs.json"))
parts = {}
for k in range(40):
    p = f"{tmp}/BedFactory_{seed}.{k}.json"
    if os.path.exists(p):
        d = json.load(open(p)); parts[f"bed#{k}"] = (np.array(d["verts"]), d["polys"])
rust0 = dict(parts)
import bpy
bpy.ops.wm.read_factory_settings(use_empty=True)
cloth_jobs.run_jobs(jobs, lambda n: parts[n], lambda n, v, f: parts.__setitem__(n, (v, f)))
for rn, pi in mp.items():
    py = np.array(json.load(open(f"{pyp}.{pi}.json"))["verts"])
    for label, src in (("rust-cloth", rust0), ("blender-cloth", parts)):
        rs = src[rn][0]
        d1 = cKDTree(rs).query(py)[0]; d2 = cKDTree(py).query(rs)[0]
        print("%-6s %-14s nv %6d/%6d mean %.2e p95 %.2e max %.2e" % (rn, label, len(py), len(rs), max(d1.mean(), d2.mean()), max(np.percentile(d1, 95), np.percentile(d2, 95)), max(d1.max(), d2.max())))

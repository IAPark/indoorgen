"""clothcheck_balloon.py SEED...: Rust-exported BalloonFactory cloth job re-simmed with Blender (tools/cloth_jobs.py) vs the cached python mesh
(~/work/refcache/BalloonFactory/<seed>_0.bin).  Prints surface NN distances for the Rust default mesh and the Blender re-sim.  Run with the infinigen venv python."""
import sys, os, json, struct, subprocess, numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cloth_jobs
from scipy.spatial import cKDTree
root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
import bpy
def cache(seed):
    b = open(os.path.join(os.environ.get("INDOORGEN_REFCACHE", os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "local-data", "refcache")), f"BalloonFactory/{seed}_0.bin"), "rb").read()
    n = struct.unpack("<I", b[4:8])[0]; h = json.loads(b[8:8 + n]); o = 8 + n
    return np.frombuffer(b, dtype="<f4", count=h["nv"] * 3, offset=o).reshape(-1, 3).astype(float)
def nn(a, b):
    d1 = cKDTree(a).query(b)[0]; d2 = cKDTree(b).query(a)[0]
    return max(d1.mean(), d2.mean()), max(np.percentile(d1, 95), np.percentile(d2, 95)), max(d1.max(), d2.max())
for seed in map(int, sys.argv[1:]):
    tmp = os.path.join(root, "tmp", f"bb{seed}"); os.makedirs(tmp, exist_ok=True)
    subprocess.run([f"{root}/target/fast/refcmp", "BalloonFactory", str(seed), "--raw", "--dump", tmp, "--cloth-json", f"{tmp}/jobs.json"], capture_output=True)
    jobs = json.load(open(f"{tmp}/jobs.json"))
    d = json.load(open(f"{tmp}/BalloonFactory_{seed}.0.json"))
    parts = {"bed#0": (np.array(d["verts"]), d["polys"])}
    py = cache(seed)
    print(seed, "rust-default mean %.2e p95 %.2e max %.2e" % nn(parts["bed#0"][0], py))
    bpy.ops.wm.read_factory_settings(use_empty=True)
    cloth_jobs.run_jobs(jobs, lambda n: parts[n], lambda n, v, f: parts.__setitem__(n, (v, f)), verbose=False)
    print(seed, "blender-resim  mean %.2e p95 %.2e max %.2e nv %d/%d" % (*nn(parts["bed#0"][0], py), len(parts["bed#0"][0]), len(py)))
    r = parts["bed#0"][0]
    print(seed, "bounds py", py.min(0).round(3), py.max(0).round(3), "resim", r.min(0).round(3), r.max(0).round(3))

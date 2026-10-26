"""nncheck.py class file.jsonl : for each entry run dumpasset (env PH passes through) and report vertex-count match, bbox diff, nearest-vertex distances (set agreement), index-aligned error"""
import sys, json, subprocess, numpy as np
from scipy.spatial import cKDTree
cls = sys.argv[1]; path = sys.argv[2]
RS = __import__("os").path.join(__import__("os").path.dirname(__import__("os").path.dirname(__import__("os").path.abspath(__file__))), "target/release/dumpasset")
worst = 0; worst_nn = 0; n = 0; cnt_ok = 0; idx_ok = 0
for l in open(path):
    j = json.loads(l)
    if "error" in j: print(j["fseed"], "py error"); continue
    subprocess.run([RS, cls, str(j["fseed"]), "/tmp/f_nn_rs.json"], capture_output=True)
    B = json.load(open("/tmp/f_nn_rs.json")); va = np.array(j["verts"]); vb = np.array(B["verts"]); n += 1
    ok = len(va) == len(vb)
    bb = max(abs(va.min(0) - vb.min(0)).max(), abs(va.max(0) - vb.max(0)).max())
    d1 = cKDTree(vb).query(va)[0]; d2 = cKDTree(va).query(vb)[0]
    nn = max(d1.max(), d2.max())
    idx = np.abs(va - vb).max() if ok else float("inf")
    polys_same = ok and j["polys"] == B["polys"]
    cnt_ok += ok; idx_ok += polys_same
    worst = max(worst, bb); worst_nn = max(worst_nn, nn)
    print("%s %d: verts %d/%d %s bbox %.2e nn %.2e idx %.2e polys %s" % (cls, j["fseed"], len(va), len(vb), "OK" if ok else "MISMATCH", bb, nn, idx, "identical" if polys_same else "differ"))
print("SUMMARY %s n=%d count-match %d polys-identical %d worst-bbox %.2e worst-nn %.2e" % (cls, n, cnt_ok, idx_ok, worst, worst_nn))

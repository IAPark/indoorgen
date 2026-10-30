"""Floor-plan refinement with the furniture in the loop: hill-climb on shared-wall moves (PLAN_NUDGE), each candidate laid out
in full and scored by tools/layout_score.py (furniture, circulation, work triangle, room shapes); a move is kept when the
penalty drops.

    uv run --with shapely tools/plan_refine.py OUT SEED [SEED ...] [--iters 8]

Writes OUT/l<seed>.json / .log for the refined house and OUT/refine.jsonl with every step.
"""
import argparse
import json
import os
import random
import subprocess
import sys

from shapely.geometry import Polygon

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from layout_score import score  # noqa: E402

BIN = os.environ.get("BIN", os.path.expanduser("~/work/infinigen-rs/target/fast/gen2"))


def run(out, seed, nudges, tag):
    dump, log = os.path.join(out, f"cand/{seed}_{tag}.json"), os.path.join(out, f"cand/{seed}_{tag}.log")
    env = dict(os.environ, SERIAL_ROOMS="1", GROUPDBG="1", LAYOUT_DUMP=dump, PLAN_NUDGE=";".join(f"{a},{b},{d}" for a, b, d in nudges))
    with open(log, "w") as f:
        r = subprocess.run([BIN, str(seed), "LivingRoom", "/tmp/lr.glb", "--config", os.path.join(out, "cfg.json")], env=env, stdout=f, stderr=subprocess.STDOUT)
    if r.returncode != 0 or not os.path.exists(dump):
        return None, dump, log
    d = json.load(open(dump))
    moved = sum(1 for l in open(log) if l.startswith("NUDGE") and l.rstrip().endswith("moved"))
    if moved < len(nudges):
        return None, dump, log
    return score(d), dump, log


def wall_pairs(d):
    """room pairs sharing 1 m or more of wall"""
    polys = {k: Polygon(v["outline"][0]) for k, v in d["room_info"].items()}
    ks = sorted(polys)
    out = []
    for i, a in enumerate(ks):
        for b in ks[i + 1:]:
            if polys[a].buffer(0.02).intersection(polys[b].buffer(0.02)).area / 0.04 >= 1.0:
                out.append((a, b))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("seeds", type=int, nargs="+")
    ap.add_argument("--iters", type=int, default=8)
    a = ap.parse_args()
    os.makedirs(os.path.join(a.out, "cand"), exist_ok=True)
    cfg = os.path.join(a.out, "cfg.json")
    if not os.path.exists(cfg):
        json.dump({"cloth": {"on": False}, "people": {"count": 0}}, open(cfg, "w"))
    rec = open(os.path.join(a.out, "refine.jsonl"), "w")
    for seed in a.seeds:
        rnd = random.Random(seed)
        (pen, why), dump, log = run(a.out, seed, [], "base")
        best = (pen, [], dump, log, why)
        start = pen
        pairs = wall_pairs(json.load(open(dump)))
        for it in range(a.iters):
            if not pairs: break
            ra, rb = rnd.choice(pairs)
            dd = rnd.choice([-0.5, -0.25, 0.25, 0.5])
            nudges = best[1] + [(ra, rb, dd)]
            res, dump, log = run(a.out, seed, nudges, f"i{it}")
            ok = res is not None and res[0] < best[0]
            rec.write(json.dumps({"seed": seed, "iter": it, "move": [ra, rb, dd], "penalty": res[0] if res else None, "kept": ok}) + "\n"); rec.flush()
            if ok:
                best = (res[0], nudges, dump, log, res[1])
        print(f"seed {seed}: penalty {start:.2f} -> {best[0]:.2f} with {len(best[1])} wall moves")
        os.replace(best[2], os.path.join(a.out, f"l{seed}.json"))
        os.replace(best[3], os.path.join(a.out, f"l{seed}.log"))


if __name__ == "__main__":
    main()

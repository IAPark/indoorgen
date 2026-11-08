#!/usr/bin/env python3
"""eras_bench.py BENCH.json OUT_DIR: compare make_eras strategies on the same homes.

BENCH.json: {"homes": [{"seed": 8001, "room": "LivingRoom"}, ...], "strategies": {"A": {partial make_eras config}, ...}, "keep": false}
Runs every (home, strategy) as its own cold make_eras process, interleaved home by home (the VM's slow and fast phases fall on every
strategy alike), keeps manifest.json + sheet.png of each run (the frames and photos are deleted unless "keep"), and writes
OUT_DIR/results.jsonl (one row per run) and OUT_DIR/summary.json (per strategy)."""
import os, sys, json, time, subprocess, shutil, statistics

HERE = os.path.dirname(os.path.abspath(__file__))


def row_of(run_dir, name, home, wall):
    """metrics of one run from its manifest (accepted or rejected)"""
    out = dict(strategy=name, seed=home["seed"], room=home["room"], wall_s=round(wall, 1))
    mans = [os.path.join(dp, "manifest.json") for dp, _, fs in os.walk(run_dir) if "manifest.json" in fs]
    if not mans:
        out.update(ok=False, why="no manifest (skipped house or crash)"); return out
    m = json.load(open(mans[0])); t = m.get("targets") or []
    refs = lambda k: sum(len(e["photos"]) for e in m["eras"] if e["era"] != k)
    out.update(ok=True, passed=m["qc"]["passed"], fails=m["qc"]["fails"], total_s=m["timing"]["total_s"], eras=len(m["eras"]),
               targets=len(t), frames=sum(len(x["frames"]) for x in t), photos=sum(len(e["photos"]) for e in m["eras"]),
               refs_per_target=[refs(x["era"]) for x in t], seen=[x["masks"]["share"]["seen"] for x in t], never=[x["masks"]["share"]["never"] for x in t],
               changed=[x["masks"]["share"]["changed"] for x in t], cells=[x["video_path"]["cells"] for x in t], paths=[x["video_path"]["family"] for x in t],
               dropped=m["qc"]["dropped_photos"], changes=[c for e in m["eras"] for c in e["changed"]],
               coverage=[x.get("ref_coverage") for x in t])
    return out


def summarize(rows):
    by = {}
    for r in rows: by.setdefault(r["strategy"], []).append(r)
    out = {}
    for s, rs in by.items():
        ok = [r for r in rs if r.get("ok")]
        walls = sorted(r["wall_s"] for r in rs)
        passed_targets = sum(r["targets"] for r in ok if r["passed"])
        seen = [x for r in ok for x in r["seen"]]
        out[s] = dict(runs=len(rs), crashed_or_skipped=len(rs) - len(ok), passed=sum(1 for r in ok if r["passed"]),
                      wall_mean=round(statistics.mean(walls), 1), wall_max=walls[-1], over_60=sum(1 for w in walls if w > 60),
                      examples=passed_targets, examples_per_hour=round(3600 * passed_targets / max(1e-9, sum(walls)), 1),
                      frames_per_hour=round(3600 * sum(r["frames"] for r in ok if r["passed"]) / max(1e-9, sum(walls))),
                      refs_per_example=round(statistics.mean([x for r in ok for x in r["refs_per_target"]]), 1) if ok else 0,
                      seen_median=round(statistics.median(seen), 2) if seen else 0, cells_mean=round(statistics.mean([x for r in ok for x in r["cells"]]), 0) if ok else 0,
                      fails=[f for r in ok for f in r["fails"]])
    return out


def main():
    cfg = json.load(open(sys.argv[1])); out = os.path.abspath(sys.argv[2]); os.makedirs(out, exist_ok=True)
    py = sys.executable; res = os.path.join(out, "results.jsonl")
    done = {(r["strategy"], r["seed"]) for r in map(json.loads, open(res))} if os.path.exists(res) else set()
    for home in cfg["homes"]:
        for name, strat in cfg["strategies"].items():
            if (name, home["seed"]) in done: continue
            rd = os.path.join(out, name, str(home["seed"])); shutil.rmtree(rd, ignore_errors=True); os.makedirs(rd)
            c = dict(strat, homes=[home]); cp = os.path.join(rd, "cfg.json"); json.dump(c, open(cp, "w"))
            t = time.time()
            with open(os.path.join(rd, "log.txt"), "w") as lf:
                subprocess.run([py, os.path.join(HERE, "make_eras.py"), cp, os.path.join(rd, "out")], stdout=lf, stderr=subprocess.STDOUT)
            r = row_of(rd, name, home, time.time() - t)
            if not cfg.get("keep"):
                for dp, ds, fs in os.walk(rd):
                    for d in ds:
                        if d.startswith(("era_", "target", "_gen")): shutil.rmtree(os.path.join(dp, d), ignore_errors=True)
            with open(res, "a") as f: f.write(json.dumps(r) + "\n")
            print("%s %s %s: %.1fs %s" % (name, home["room"], home["seed"], r["wall_s"], "pass" if r.get("passed") else (r.get("fails") or r.get("why"))), flush=True)
    rows = [json.loads(l) for l in open(res)]
    sm = summarize(rows); json.dump(sm, open(os.path.join(out, "summary.json"), "w"), indent=1)
    for s, v in sm.items(): print(s, json.dumps({k: v[k] for k in v if k != "fails"}))


if __name__ == "__main__":
    main()

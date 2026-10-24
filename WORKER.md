# Worker guide (read first, it is short)

Project: Rust port of Infinigen Indoors scene generation (`~/work/infinigen-rs`, python reference `~/work/infinigen-ref`,
venv python `~/work/infinigen-ref/.venv/bin/python`, bpy 4.2). Goal: Rust scenes as good as the reference; assets should
reproduce the python meshes (surface-equal is the bar, exact vertex order is a bonus) and placeholders/bboxes exactly.
Background docs: `FRAMEWORK.md` (geometry-nodes interpreter), `BACKLOG.md` (work items), `DONE-*.md` in git history.

## Your sandbox
- You work in a git worktree `~/work/wt/<you>` on branch `work/<you>`. Never edit `~/work/infinigen-rs` (master) or other worktrees.
- Commit early and often (`git commit -am ...`, small commits with clear messages). Uncommitted work is lost if you are stopped.
- To pick up other workers' merged work: `git merge master` (only when your tree builds; resolve conflicts keeping both sides' intent).
- Temp files: `export TMPDIR=~/work/wt/<you>/tmp` (gitignored); keep it under 300 MB and delete what you no longer need.
  Disk is shared and small.

## Fast loop (seconds, not minutes)
- Build only what you run: `. ~/.cargo/env; CARGO_BUILD_JOBS=2 cargo build --profile fast --bin refcmp` (~20 s per edit).
  Never build all 60 bins; never use `--release` unless timing.
- Asset check WITHOUT Blender: `target/fast/refcmp <PythonClassName> [seeds]` compares the Rust factory output with
  cached python meshes in `~/work/refcache/<Class>/<seed>_<inst>.bin`. If a class/seed is missing, cache it once:
  `~/work/infinigen-ref/.venv/bin/python tools/refcache.py module.Class:0-5` (module relative to infinigen.assets.objects;
  see tools/refcases.txt). Prints PASS/FAIL, counts, order error, surface mean/p95/max, bbox, timings.
- Node-graph assets: `tools/gn_check.py` (see FRAMEWORK.md; `--bisect` finds the first wrong node; results are cached).
- Placeholders: `target/fast/phcheck <oracle.jsonl>` (oracles: `~/work/ref/ph_LR3_hs0b.jsonl`, `~/work/ref/traces/case_*_ph.jsonl`).
- Whole pipeline gate before you finish: `tools/selfcheck.sh` (builds, determinism, lockstep placement, phcheck). It must stay green.
- RNG order of a python factory: `~/work/ref/scripts/rnglog.py module.Class seed`.
- Only ONE python/Blender process at a time (4 shared cores).

## Rules that bit us before
- Use `crate::dhash::{HashMap, HashSet}` (`HashMap::default()`), never std RandomState maps (nondeterministic output).
- Reuse before writing: bevel = `src/bm/bevel.rs` (exact bmesh bevel) / `src/assets/bevelfull.rs` (modifier); bridge loops =
  `src/assets/bridge.rs`; fill_grid/solidify = `src/assets/bpyops.rs`; subdivision = `src/subdiv.rs`, gn SubdivisionSurface;
  CDT = `src/gn/cdt.rs`; booleans = `src/gn/csg.rs`. If something is missing in shared code, extend it there (with a test),
  don't fork a private copy.
- Keep asset code in its own file; touch `src/assets/mod.rs` only to add `pub mod` + one dispatch arm.
- Don't stop at a "first pass". Work through your backlog items until done or genuinely blocked; update `BACKLOG.md`
  (status + measured numbers + next step) as you go and commit it.
- Final reply: <= 15 lines: what is done (refcmp numbers), what is not, branch name, last commit.

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
- The code is a Cargo WORKSPACE: crates under `crates/` (see `crates/README.md` for which crate holds what). Edit files there; an edit
  rebuilds only its crate + dependents (asset-family edits ~2 s, gn/core ~5 s on an idle machine).
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

## Debugging one op (Blender op vs the Rust port, seconds per iteration)
- `tools/opsuite.sh [-b] [-f] [-v] [id-substr...]` runs the regression table `tools/opcases/suite.txt` (63 cases, Blender results cached in
  `tools/opcases/<id>.blender.json`, so no Blender needed on reruns; `-f` re-runs Blender). Levels: exact > verts (vertex order exact, face
  order differs) > surface (same surface, any order) > none; each line has a floor so the suite is green unless something regressed.
- One op by hand: `~/work/infinigen-ref/.venv/bin/python tools/opcheck.py bevel --in tools/opcases/lshape.json --params '{"offset":0.05,"segments":2}' --out /tmp/r.json`
  (`--list` shows ops: bevel, bevel_mod, solidify, bridge, fill_grid, subsurf, subdivide, boolean, triangulate(_op), beautify_fill, remove_doubles,
  remesh, displace, convex_hull, and generic `modifier`), then `target/fast/opcheck bevel --in tools/opcases/lshape.json --params '...' --blender /tmp/r.json`.
  It prints counts, exact-order max error, face identity, NN vertex match, sampled surface distance and the FIRST differing vertex/face with its
  neighbourhood (indices + coords of both meshes). Exit 0 ok / 1 mismatch / 2 unsupported by the Rust port. Case format: header of tools/opcheck.py.
- Add an op: python `def op_x(c,p)` + OPS entry in tools/opcheck.py, Rust `fn r_x(&Case,&Value)->Res` + row in OPS of src/bin/opcheck.rs.
  Add a case: `tools/opcases/gen.py` + a line in suite.txt (`id|op|case|params|[floor]`).
- Whole asset, per stage: `python tools/stagedump.py seating.chairs.chair.ChairFactory 0 tmp/sd` dumps the mesh after every bpy.ops.mesh / modifier
  apply / join (+ the BEFORE mesh and selection as an opcheck case). Pick the first stage where the Rust asset diverges, then
  `target/fast/opcheck <op> --in tmp/sd/NNN_*.case.json --blender tmp/sd/NNN_*.json` shows which op port is wrong and where. (Output is big: keep it in tmp/.)
- Blender orders the edges of a mesh by its own rule; opcheck passes the recorded `in_edges` to the Rust bevel so the vertex order can match.

## Linking C/C++ libraries is allowed (user decision, 2026-10-24)
- Where exactness depends on a library that Blender/python itself uses, LINK it instead of porting: e.g. OpenSubdiv 3.6.0 (Blender 4.2's
  subdivision), libc `qsort` (macOS, unstable; decides some Blender ties), OpenVDB (voxel remesh), libccd/FCL (collision), Eigen via a small
  C++ shim. Use the `cc` crate (add to Cargo.toml) for C/C++ shims under `crates/ig_core/csrc/` (built by crates/ig_core/build.rs), or link Homebrew/system libs; put sources/tarballs under
  your tmp/ or a pinned path, not in the repo if large (>5 MB). Match the version Blender 4.2 / the python wheels use, and the compile flags
  that matter for float results (arm64 clang contracts a*b+c into FMA by default: `-ffp-contract=on`; that is how python-fcl behaves).
- Keep a pure-Rust path only if a port already exists and is exact.
- Blender itself (bpy or its internals) is ONLY the reference/test oracle (refcache, opcheck, gn_check, the bed cloth sim at render time). Never call or link Blender from generation code (user decision 2026-10-24).

## Crate ownership (user decision 2026-10-24)
- You own only the crate(s) named in your assignment. Before changing ANY file in another crate (shared crates ig_core, ig_bm, ig_exbool,
  ig_geom, ig_kit, ig_gn, ig_shared, ig_furn, ig_rooms, ig_assets, or another worker's family crate) or Cargo.toml / build.rs / tools used by
  others: message the orchestrator first (Claude Code Remote send_message, session_id "@parent") with the file, the change and why, and wait
  for an OK before committing it. Adding a dispatch arm for your class in crates/ig_assets/src/lib.rs is pre-approved.

## Rules that bit us before
- Placeholder / bounding-box geometry must be float32-bit-exact vs python (`PHTOL=1e-12 target/fast/phcheck <oracle>`):
  the solver's lockstep depends on it (1e-7 differences still change placement). Treat it as part of "done" for any asset.
- If you delegate to a sub-agent, give it its OWN worktree (`tools/newworker.sh <name>`); two agents in one worktree interleave commits.
- macOS tools: BSD `sed -i ''` (not GNU), no `timeout`; prefer small python edits for multi-line changes.
- Object counts per room are chaotic per seed; judge solver/room changes with `tools/countbench.sh` (means over all rooms).
- Use `crate::dhash::{HashMap, HashSet}` (`HashMap::default()`), never std RandomState maps (nondeterministic output).
- Reuse before writing: bevel = `crates/ig_bm/src/bevel.rs` (exact bmesh bevel) / `crates/ig_kit/src/bevelfull.rs` (modifier); bridge loops =
  `crates/ig_kit/src/bridge.rs`; fill_grid/solidify = `crates/ig_kit/src/bpyops.rs`; subdivision = `crates/ig_core/src/subdiv.rs`, gn SubdivisionSurface;
  CDT = `crates/ig_gn/src/cdt.rs`; booleans = `crates/ig_gn/src/csg.rs`. If something is missing in shared code, extend it there (with a test),
  don't fork a private copy.
- Keep asset code in its own file; add `pub mod` to your crate's lib.rs and one dispatch arm in `crates/ig_assets/src/lib.rs` (make_factory).
- Don't stop at a "first pass". Work through your backlog items until done or genuinely blocked; update `BACKLOG.md`
  (status + measured numbers + next step) as you go and commit it.
- Final reply: <= 15 lines: what is done (refcmp numbers), what is not, branch name, last commit.

# Geometry Nodes framework (crates/ig_gn, tools/gn_*.py, src/bin/gncheck.rs)

Goal: port the ~330 infinigen assets that build geometry with `nw.new_node(Nodes.X, ...)` without hand-translating each graph.
The graph is exported from Blender 4.2 as JSON, a Rust interpreter evaluates it (float32, Blender semantics), and an automatic
checker compares the interpreter with Blender's own evaluation (vertices, faces, attributes) on unit graphs, on real
`@to_nodegroup` groups and on whole asset factories.

## Layout

| path | what |
|---|---|
| `tools/gn_export.py` | bpy node group -> JSON (`group`, `asset`, `template` modes), recursive for nested groups |
| `tools/gn_check.py` | checker driver: `unit`, `group`, `asset` modes (+ `--bisect`); builds case files, runs `target/release/gncheck` |
| `tools/gn_units.py` | per-node unit graphs (python/bpy); `gn_check.py unit [filter]` |
| `tools/gn_survey.sh` | run `gn_check.py asset` over a list of factories (sequential, 240 s watchdog) |
| `crates/ig_gn/src/` | the interpreter (below) |
| `src/bin/gncheck.rs` | evaluates a case file with the interpreter and compares with Blender's result |
| `crates/ig_shared/src/gnasset.rs`, `crates/ig_storage/src/gnports.rs` | node-graph assets as `Factory` (template JSON in `data/gn/` + RNG-exact sampler) |

`crates/ig_gn/src/` modules: `val.rs` (socket value types, Ty/Val), `geom.rs` (Mesh / Curves / Instances / Points, attribute maps on
domains), `field.rs` (lazy fields: `FNode`, `Ctx`, kernels), `attr.rs` (domain interpolation = Blender "adapt domain",
normals), `load.rs` (JSON -> `Group`), `eval.rs` (demand-driven evaluator, memoised per node, nested groups, probes),
`nodes*.rs` (node implementations, dispatched by `bl_idname`), `meshops.rs` (join/transform/realize/keep), `curveops.rs`
(evaluated points, tangents, normals, lengths, bezier / catmull-rom), `prim.rs` (primitive layouts), `noise.rs` (Perlin
fBm, Voronoi, white noise: exact Blender hashes), `csg.rs` (mesh boolean, BSP), `rot.rs`, `math.rs` (Math/VectorMath
float32 ops, hashes).

## Exporting a node group

```
PY=~/work/infinigen-ref/.venv/bin/python
$PY tools/gn_export.py group infinigen.assets.objects.tables.table_utils.nodegroup_n_gon_profile out.json
$PY tools/gn_export.py asset tables.TableDiningFactory 3 out.json     # every geometry-nodes modifier applied while building seed 3
$PY tools/gn_export.py template shelves.triangle_shelf.TriangleShelfFactory 0 1 data/gn/triangle_shelf.json
```

JSON: `groups{name:{inputs,outputs,nodes,links}}`; nodes keep `bl_idname`, properties (`p`), enabled sockets with ids and
default values; links carry `multi_input_sort_id` (Join Geometry takes them in REVERSE order). Frames are dropped; reroutes,
muted nodes and nested group nodes are kept. `template` exports the asset for two factory seeds and turns every value that
differs into a *patch* (`param` name from the factory's `add_geomod(input_kwargs=...)`); unmatched differences are listed
as `unmapped`.

## Checking against Blender

```
$PY tools/gn_check.py unit [filter...]            # ~465 unit graphs; filter = substring of "group:Name" (e.g. "math:" "Voronoi")
$PY tools/gn_check.py group MODULE.FUNC [--n 4] [--bisect]
$PY tools/gn_check.py asset MODULE.CLASS SEED [--bisect]
options: --keep --cases PATH --tol 1e-6 --filter NAME --show (unit: print Blender's mesh) ; env GNDUMP=1 prints the Rust mesh on failure
```

Per case: `CASE PASS name maxerr=...` (exact vertex/face order, attributes to tol), `PASS ... SURFACE-EQUAL` (same surface,
different index order; only allowed when no attribute differs), or `CASE FAIL name <first differing vertex/face/attr>`.
`--bisect` re-evaluates every geometry-valued node output (also inside nested group instances) in Blender and Rust and prints
the first node whose output differs ("bisect: ... first failing: node 'X' (GeometryNodeY)").
Comparison of materials is by name (slot numbering differs after modifier apply).

## Adding a node

1. Implement it in the matching `crates/ig_gn/src/nodes_*.rs` match arm (`outs(node, vec![("SocketId", value)])`; fetch inputs with
   `ev.field/ev.geo/ev.cf/ev.ci/ev.cv`, socket ids are the Blender *identifiers*: see `tools` dump in `/tmp` or
   `n.inputs[i].identifier`). Function-style nodes build a kernel with `mf1/mf` (fields stay lazy); geometry nodes work on `Rc<Geometry>`.
2. Add unit graphs to `tools/gn_units.py` (use `base_grid`, `field_case` helpers) and run `gn_check.py unit <name>`.
3. Fix until `PASS`; edge-order differences are reported as `EDGE-ORDER-DIFFERS` (not a failure).

## Porting an asset

1. `gn_export.py template ...` for two seeds; check `unmapped` (derived parameters must be re-computed in the sampler).
2. Port `get_asset_params` draw-for-draw (`inst_rng(factory_seed, inst)` = `FixedSeed(int_hash((factory_seed,i)))`; verify with
   `~/work/ref/scripts/rnglog.py`), register the class in `crates/ig_storage/src/gnports.rs` (+ `handles`), data file in `data/gn/`.
3. `gn_check.py asset MODULE.CLASS SEED --bisect` must pass; surface check of the final asset with `gc.sh` as usual.

## Known gaps / deviations (see DONE.md)

NURBS evaluation, Poisson point distribution, Raycast, DualMesh, shortest-path nodes, volume nodes, images, Voronoi 1D/2D/4D,
bezier auto-handle recomputation, `Convex Hull` (Blender perturbs hull vertices by ~1e-4), `Sample Nearest` tie-breaking,
Mesh Boolean is BSP based (surface-equal, not vertex-equal). Primitive edge order is only replicated for Grid/Cylinder/CurveToMesh.

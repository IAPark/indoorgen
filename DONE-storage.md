# DONE.md (storage agent): shelves / appliances / bathroom / cookware ports

Crate copy: ~/work/agents/storage/infinigen-rs. Tools live in ~/work/agents/storage/ (gc.sh, geomcmp.py, pydump.py, surfcmp2.py, jcmp.py, fontmetrics.py, ...).
Exactness tooling (new):
- `python pydump.py MODULE.Class SEED0 N INST OUT.jsonl [asset|ph|both]` : python spawn_asset / placeholder verts+polys+face tag names (materials stubbed)
- `target/release/rsdump CLASS SEED0 N INST OUT.jsonl` : same from `Factory::exact` ; `python jcmp.py py.jsonl rs.jsonl` : vertex count, max err, #vertices not bit-equal(f32), polys identical?, names identical?
- `target/release/meshdiff DUMP.jsonl [asset|ph|populated]` : same comparison per class against oracle files (ph = placeholders, populated = real-scene assets incl. canonical tag names + final triangulation)
- `surfcmp2.py` : float64 exact surface distance. NOTE: gc.sh/geomcmp.py (mathutils BVH, float32) is NOISE-LIMITED for sliver triangles (reported 5e-4..9e-4 errors on meshes that are in fact exact); use surfcmp2.py or jcmp.py.

## Verified BIT-EXACT assets (vertex count, order, float32 values, polygon lists, face tag names), many seeds, python spawn_asset(inst)
LargeShelfFactory, SimpleBookcaseFactory, CellShelfFactory, TVStandFactory, SimpleDeskFactory, SidetableDeskFactory (8-12 seeds each),
KitchenCabinetFactory (6 seeds, ~20k-74k verts), SingleCabinetFactory (8 seeds), RugFactory (12 seeds, all 4 shapes; face_names empty like python).
Real-scene oracle ("populated" lines of ~/work/ref/ph_LR3_hs0b, traces/case_LivingRoom_1, case_Bedroom_0): CellShelf 3/3, SimpleBookcase 5/5, LargeShelf 1/1, KitchenCabinet 2/2, SingleCabinet 3/3 identical in verts (bit), polys and names (after `ExactMesh::triangulated()` [edit-mode BEAUTY triangulation] + `with_canonical_tags()`).
Ported Blender internals (exact): Mesh cube/cylinder primitives (float32, accumulated angle), Transform (euler->quat->mat, mul_m4_v3), Join (reverse link order), Triangulate (shortest diagonal quads, BLI_polyfill + beautify for ngons, BEAUTY quad), Curve to Mesh, Merge by Distance (full weld port), Subdivide Mesh simple level N (OpenSubdiv layout + FMA bilinear eval), FlipFaces, BMesh circle (sin_cos_from_fraction), font metrics for StringToCurves.
Files: src/assets/{cabnodes,cabdoor,gweld,polyfill,aquarium,fontdata}.rs (new), shelves.rs, appliances.rs, bathroom.rs, cookware.rs (edited).

## Placeholders (python create_placeholder / bbox-of-asset) vs oracles (phcheck-style, |d|<1e-5, plus box layout/polys/names via meshdiff ph)
Exact (<1e-6, layout+names identical): Rug 157/157, KitchenCabinet 52/52, LargeShelf 23/23, SimpleBookcase 24/24, CellShelf 66/66, TVStand 41/41, SimpleDesk 50/50, SingleCabinet, KitchenSpace 305/305 (two-box layout with support/top names), Aquarium 30/30 (new port), Monitor 5/5, Oven 8/8 (+30 random seeds; panel text bounds from glyph metrics).
Within 5e-6: KitchenIsland 141/141 (bbox of asset = exact cabinet + countertop slab bounds fitted: c=0.49992449*range+0.0005002; residual from slab fillet), Dishwasher/BeverageFridge (formula verified on 40 seeds, 5e-6).
Verified vs python on random seeds (no oracle in files): TV, Toilet (fixed: extra RNG draw removed and bogus 90deg rotation of placeholder removed), BathroomSink, StandingSink, Bathtub bounds_hint exact for alcove (~50% of seeds, rest falls back to approximate asset), Microwave bounds_hint exact on ~75% of seeds (python's own bevel creates stray vertices on the rest: bbox up to +-0.3 m outside; not reproducible without Blender bevel).

## NOT exact (approximate asset surfaces, unchanged from before unless listed)
Oven/Dishwasher/BeverageFridge/Microwave assets (node graphs with booleans, bevel, text, racks), TV/Monitor assets (boolean/bevel/bridge loops), Toilet/Bathtub/BathroomSink/StandingSink/Hardware assets (modifier based), Pan/Pot, KitchenSpace/KitchenIsland assets (countertop = table_top.py fillet/curve graph, top cabinets; only bounds ported), Aquarium asset (tank only; coral/plant inside not ported), Hardware placeholder uses wrong RNG (Rng::seeded(i+3) for ring minor radius).
Open: door mesh positions bit exact only because OSD eval uses FMA (mode 9); other OSD cases (adaptive) untested. SingleCabinet/KitchenCabinet exactness depends on shader draw counts in shelf_material_draws (white/black_wood/wood shaders draw 0, rough_plastic 5 doubles, shader_wood choice(49)+9).

## Shared-file diffs for the orchestrator
src/assets/mod.rs: `pub mod cabnodes; pub mod polyfill; pub mod aquarium; pub mod fontdata; pub mod gweld; pub mod cabdoor;`; make_factory: `c if aquarium::handles(c) => aquarium::make(c, &mut rng),` before shelves arm... (after shelves); Factory trait gained default methods `exact(inst)->Option<ExactMesh>`, `placeholder_exact(inst)`, `bounds_hint(inst)->Option<(V3,V3)>`; `bbox_of_asset()` helper used by RealAssets::placeholder for BboxOfAsset; `Asset::bbox_exact`. New bins: meshdiff, rsdump, probecmp, ngoncmp, rugprobe, doordump, drawerdump, ngdump, hintcmp, dbgbounds, dbgoven, phbounds, popdiff (debug only).
No changes to src/furn, mesh.rs, geomkit.rs.

# PHASE 2 (second assignment) -- measured results (2026-10-24)
Tools: `bbcheck PYASSET.jsonl` (bounds of python spawn_asset vs Rust `bbox_of_asset`, tol 1e-5), `phcmp PH.jsonl` (Rust `placeholder()` bounds vs python placeholders), `phcheck` now prints max|d| in %e. Stage dumpers: ~/work/agents/storage/{panstages,bathstages,micro_probe,setcmp}.py.

## Bit-exact meshes
- HardwareFactory: hook/holder/bar/ring, all 24 seeds x inst 0 and 3: vertex count, polygon lists identical; hook/holder/bar vertices bit-equal (f32); ring (torus 128x12 bpy operator + SUBSURF 2 in Blender/OSD layout, 24704 verts) max err 9e-8 (limit-surface eval noise). RNG bug fixed (ring minor radius drawn from inst_rng(fseed, inst)). New: hwexact.rs (bpy cylinder/cube/torus, transform_apply f32, join, calc_edges with 4 hash maps = machine thread count dependent!, regular B-spline limit eval), gweld::subdivide_ev (OSD layout + custom evaluator).

## Placeholders / bbox-of-asset (python vs Rust, 1e-5)
- Bathtub (freestanding+alcove): 100 seeds inst 0 + 6 seeds inst 5: all exact, worst 1.0e-6 (was 16/30). New geometric port bathfree.rs (bowl simple+CC, remove_top, rim extrude/resize, SIMPLE_DEFORM taper/stretch (default axis X), SOLIDIFY, SUBSURF 2+1 limit). Vertex order NOT Blender's.
- Pan: 80 seeds (inst 0 and 3): all exact, worst 6.8e-7 (was 0/30, 2-6 cm off). pangeom.rs (cage, handle extrusions, solidify offset +1, ExtrudeMesh region guard, SUBSURF simple1+CC3, hole cut via 32-gon prism bounds). Cap is a fan, flat plateau z=-thickness assumed.
- Pot: placeholder() added (was missing -> empirical fallback): 40/40 exact, 1.6e-8; fixed Pot has_handle=False.
- SinkFactory (kitchen sink, previously unported): new ksink.rs, placeholder 40/40 exact (3e-8); asset approximate.
- KitchenIsland: slab now from tableparts::table_top_exact: 141/141, max 9.4e-7 (was 5e-6). KitchenSpace 305/305 2.4e-7, KitchenCabinet 5.8e-8.
- Toilet, BathroomSink, StandingSink, TV, Monitor placeholders: 40 seeds each, worst 1.1e-7 (bounds; python raw quads vs our triangulated layout differ only in poly list).
- Dishwasher, BeverageFridge bbox: 30/30, 2e-7. Oven bbox-of-asset is not used (PlaceholderBBox -> placeholder, 8/8).
- phcheck on 4 oracle files: no regression (remaining non-exact: Coffee/SideTable, Jar = other agents).

## NOT done / infeasible
- Microwave: 26/30 exact bbox; 4/30 python bbox contains stray bevel vertices (up to 0.27 m). Cause found: preprocess and final node-graph meshes differ in topology (Exact MeshBoolean/ConvexHull output order), so `bevel_edges` indices from the first mesh select wrong edges in the second. Needs Blender's mesh_boolean + bmesh bevel ports; not attempted.
- Meshes (not bounds) of Oven, Dishwasher, BeverageFridge, Microwave, TV/Monitor, Toilet, Bathtub, BathroomSink, StandingSink, Pan, Pot, KitchenSpace/Island, Aquarium contents remain approximate: need bmesh bevel (7.9k lines, fetched to /tmp/stor_src2/bmesh_bevel.cc), exact Boolean, CDT (FillCurve text), font outlines, bridge_edge_loops/fill_grid ports.

## Shared-file diffs (src/assets/mod.rs)
`pub mod hwexact; pub mod bathfree; pub mod pangeom; pub mod ksink;`; make_factory: `c if ksink::handles(c) => ksink::make(c,&mut rng,factory_seed)` before bathroom arm; bathroom/cookware/appliances/aquarium `make` now take `factory_seed` as 3rd arg (call sites updated). gweld.rs: subdivide -> subdivide_ev. src/bin: bbcheck, phcmp, panone, torcmp. phcheck.rs: output format only.

# PHASE 3 (GN framework assignment) -- measured results (2026-10-24)

## Framework changes (src/gn, tools)
- `StringToCurves` (src/gn/nodes_text.rs + data/gn/glyphs.json: Bfont glyph outlines/advances incl. ':'), unit cases Text.*.
- `FillCurve` TRIANGLES: exact port of Blender's constrained Delaunay (src/gn/cdt.rs, delaunay_2d.cc incl. Shewchuk predicates); NGONS keeps the old path.
- Nodes acting on geometry now recurse into instance references (override mechanism in eval.rs/nodes.rs); `SetPosition` translates instances; `BoundingBox` keeps the instance structure; realize order = mesh, points, instances, curves. Units: DuplicateElements.INSTANCE.*, BoundingBox.instances.rot.
- tools/gn_export.py exports `final` (mesh of the finished asset after add_bevel); tools/gn_check.py `factory` now compares against it (geometry only); tools/gn_slim.py slims a `template` export to the last two steps (preprocess, final).

## Bevel port (src/bm: mod.rs BMesh with Blender element order, query.rs, bevel.rs)
Full port of bmesh_bevel.cc for edge bevel (offset type OFFSET, superellipse profiles, M_POLY/M_ADJ/M_TRI_FAN vertex meshes, loop_slide, sharp miters, adjust_offsets; NOT ported: seams/UV, harden normals, vertex bevel, custom profile, clamp_overlap, cutoff vmesh).
`bevtest` (src/bin/bevtest.rs) vs `bpy.ops.mesh.bevel` on 21 cases (cube all/partial edges 1-8 segments, profiles 0.3-0.9, L-shape, wedge, pyramid, ridge): 21/21 bit-identical vertex AND face order, max diff 0.
Files: ~/work/agents/storage/bevtest/{gen.py,cases.json}.

## Exact via `gn_check.py factory` (vertex/face order exact, maxerr ~2e-7)
- DishwasherFactory seeds 0-6, BeverageFridgeFactory seeds 0-6, OvenFactory (electric, use_gas=0) seeds 1,2,3,4,5,7: all PASS (new src/assets/bevelgn.rs: preprocess/final graph steps + python get_bevel_edges + add_bevel).
- Gas ovens (use_gas=1) and MicrowaveFactory: node graph + bevel wired but NOT exact (Mesh Boolean FLOAT solver differs from Blender: 2134 vs 960 verts at 'Mesh Boolean.001'; Blender's result is not even deterministic run to run); gas ovens fall back to the old hand asset, microwave uses the GN+bevel result (approximate). Gas grates (Boolean + bevel modifier) not ported.

## Shared-file diffs
src/lib.rs: `pub mod bm;`. src/assets/mod.rs: `pub mod bevelgn;` and make_factory arm `c if bevelgn::handles(c) => bevelgn::make(c, &mut rng, factory_seed),` (after gnports). Placeholders/bounds_hint are delegated to the previous implementation (unchanged).

## Unit suite after the framework changes
`gn_check.py unit`: pass=611 fail=10. The 10 are known gaps (NURBS fac x2, SampleNearest.FACE ties, ConvexHull x2, AlignEulerToVector x2 and DistributePointsOnFaces rot x2 sign/angle-wrap, tex:VectorCurve); none touch Text/FillCurve/instance recursion/BoundingBox.

## NOT done in this phase
TV/Monitor, Toilet, Bathtub, BathroomSink/StandingSink, Pan/Pot extras, KitchenSpace/Island slab+top cabinets, Aquarium contents: these are built with bpy ops/modifiers (BEVEL modifier, BOOLEAN, bridge_edge_loops, solidify, geo_radius) rather than node graphs, so the template framework does not apply; they keep the phase-1/2 results (exact placeholders/bboxes; Hardware, Pan, kitchen Sink, KitchenIsland slab exact earlier). The bevel port in src/bm is the building block for the BEVEL-modifier ones (needs the modifier's ANGLE limit method + width offset type on top).

# DONE.md - furniture agent (tables / seating)

Tools (outside crate): ~/work/agents/furniture/tools: pybatch.py (python dump of spawn_asset, stubbed materials, PH=1 -> create_placeholder),
chk.sh (python dump + `diffasset`: vertex count, max coord err, polygon lists identical), chk2.sh/nncheck.py (set agreement via nearest vertex, bbox),
dd.sh/cmpdump.py (single seed), pyfc.py/pyalign*.py/pycr.py/pygrp.py/pysofa.py/pytree.py (bpy probes). New bins in crate: dumpasset, diffasset, dumppart, phtab, fctest, crtest.
Usage: `tools/chk.sh tables.dining_table.SideTableFactory SideTableFactory 1 60` ; `PH=1 tools/chk2.sh seating.sofa.SofaFactory SofaFactory 1 16`.
Face tag names were NOT compared (geometry/topology only).

## Status per class (60 seeds unless noted; "polys identical" = vertex-index polygon lists equal to python)
| class | placeholder (oracle phtab, all 4 files) | asset vertex count | polygon lists | worst coord err |
|---|---|---|---|---|
| SideTableFactory | 181+102+41 exact | match | identical | 1.0e-6 |
| CoffeeTableFactory | 35+28 exact | match | identical | 1.4e-6 |
| TableDiningFactory | (no oracle) bbox-of-asset | match | identical | 1.9e-6 |
| TableCocktailFactory | placeholder = bbox of 10-pt filled profile x height (bounds only; python mesh is a 30-vert prism, not reproduced; no oracle entries) | match | identical (asset) | 2.3e-6 |
| OfficeChairFactory | 110+125+130 exact | match (stand and wheeled legs) | identical | 1.7e-7 |
| BarChairFactory | (no oracle) | match | identical | 5e-7 |
| SofaFactory | 83+72 exact (bbox of node-group mesh, as python PlaceholderBBox) | match (placeholder and hipoly) | DIFFER (index order) | set agreement: nearest-vertex <= 3.3e-6, bbox <= 4.4e-7 |
| ArmChairFactory | no oracle; same code | match | DIFFER | nn <= 3.3e-6 |
| BedFactory | 32 exact (pre-existing placeholder, untouched) | not attempted | - | - |
| ChairFactory | not touched (pre-existing approximation) | no | no | - |

Coordinate residuals 1-2e-6 on tables are float32 noise: python's FilletCurve arc points are computed in float32 with an unknown op order
(geometry identical to 1e-7, but not bit-exact), which perturbs the resampled top profile by <=2e-6. Everything else in the table/chair chain was brought to
bit-exact float32 (n-gon profile + resample are bit-identical; CurveCircle uses double sin/cos; Transform node matrix goes through a quaternion; resample factor uses (x-prev)*(1/(L-prev)); Catmull-Rom uses t=j*(1/res)).

## Why some classes are not topology-exact
- Sofa/ArmChair: final SUBSURF (OpenSubdiv) orders vertices by Blender's internal edge order of every intermediate mesh (Extrude, SubdivideMesh, primitives); not reproducible without that source.
  Counts match; surface matches (limit-surface SUBSURF modifier emulated via CC level 1 + limit mask).
- Bed: asset includes cloth simulation (physics), pillows, towels: infeasible. Chair: bpy.ops bridge_edge_loops/fill_grid + BEVEL/SOLIDIFY modifier output order: not attempted.

## Key findings (python quirks reproduced)
- FloatCurve: CurveMapping table (256), auto handles w/o the F-curve 5x clamp, end extrapolation along handles, node output clamped to [0,1].
- Join Geometry output order = reverse of python link list; legs group = [stretchers, legs], then seat/top last.
- Min-twist normals with fallback (1,0,0) for vertical tangent (threshold 1e-5); align-to-vector pivot degenerate case = 60 deg turn; antiparallel = half turn.
- Zero-length stretchers (Strecher Increament 0) still emit flat discs. Wheeled leg ignores 'Leg Wheel Arc Sweep Angle' (default 240 used).
- Map Range vector clamps centering to [0,1]; float->int socket conversion truncates.

## Changes outside owned files (for the orchestrator to merge)
src/assets/mod.rs:
```
 pub mod seating;
+pub mod curvekit;      // new file: FloatCurve, poly curves, fillet, resample, curve_to_mesh, catmull-rom, bezier segment
+pub mod officechair;   // new file: OfficeChair, BarChair, curvy seat, wheeled leg, capped cylinder
+pub mod sofa;          // new file: Sofa, ArmChair
 ...make_factory:
+        c if sofa::handles(c) => sofa::make(c, &mut rng),
+        c if officechair::handles(c) => officechair::make(c, &mut rng, factory_seed),
         c if stools::handles(c) => ...      // stools::handles now matches nothing (src/assets/stools.rs edited)
```
New files: src/assets/{curvekit,officechair,sofa}.rs; rewritten: tables.rs, tableparts.rs (old helper fns kept, seating.rs unchanged but Sofa/ArmChair now dispatch to sofa.rs first).
New bins: src/bin/{dumpasset,diffasset,dumppart,phtab,fctest,crtest}.rs.
Caveat: BboxOfAsset classes now build the full mesh to get bounds (~20-60 ms for tables, more for sofas/wheeled chairs).

## Update: second assignment (Chair, Sofa index order, doors/windows, remaining approximations) - SURVEY ONLY, nothing new ported
No code changed for this assignment. Findings:
1. ChairFactory (seating/chairs/chair.py, 459 lines): built with bpy.ops (bezier -> MIRROR modifier -> edit-mode fill_grid -> SOLIDIFY -> SUBSURF -> BEVEL modifiers; bridge_edge_loops with number_cuts=64 for back panels; extrude_edges_move; geo_radius nodes).
   Exact index order would need ports of Blender's fill_grid, bridge_edge_loops, SOLIDIFY and BEVEL(segments=8) output layouts; none of these exist in the crate (subdiv.rs bevel is surface-level only). Not started; the existing seating.rs Chair stays an approximation (mean surface error ~8 mm per earlier measurement).
2. Sofa/ArmChair index order: OpenSubdiv output order = [coarse verts, edge points in the coarse mesh's *edge array order*, face points]. Edge array order of every intermediate mesh (MeshCube explicit edges, Extrude Mesh, Scale Elements, Subdivide Mesh, joins) is needed; src/assets/subsurf.rs (decor) states its output order is not Blender's, and cabnodes tracks explicit edges only for primitives before Subdivide Mesh. Not feasible without porting every op's edge order; surface/bbox/counts already match (see table above).
3. Doors / windows (never ported in this crate): python sources are
   objects/elements/doors/{__init__,base(1282 lines),panel,lite,louver,casing,bar_handle(631),joint_utils(2010)}.py (DoorFactory, PanelDoorFactory, GlassPanelDoorFactory, LiteDoorFactory, LouverDoorFactory, DoorCasingFactory, BaseDoorFactory) and objects/windows/window.py (WindowFactory, 1763 lines); sim_objects/door*.py are articulated-object variants.
   ~6000 lines of node graphs + bmesh edits; not started. Suggested path: reuse curvekit.rs/pytree.py (node listing tool) and the mesh-primitive layouts in sofa.rs (mesh_cube/grid/cone/cylinder) per door part.
4. Still approximate in src/assets (non-exhaustive): proxy.rs (fallback recipes for classes without a port), plants.rs (simplified small_plants/cactus/mushroom), aquarium.rs, seating.rs Chair and Bed, plus anything the registry resolves through proxy::proxy_asset.

## Update: doors, windows, chair (relaxed standard: RNG order + params exact, surface-exact geometry, exact bbox)

mod.rs diff (mine only): `pub mod solid;` `pub mod doors;` `pub mod windows;` (plus earlier curvekit/officechair/sofa).
matdraw.rs diff (additive, minimal): `call()` gained arms "lamp_shaders.LampShade" (U(.03,.08),U(.5,.8),U(.05,.2)) and "fabric.FineKnitFabric" (3x U(.2,1), U(0,1), U(100,300), U(.75,1.33), U(.001,.005)).
solid.rs: chamfer() now merges the two candidate corner points on a shared unselected edge when they differ by <0.2 leg (Blender uses one BoundVert); no clamp when the leg exceeds the edge (Blender extrapolates). Door results unchanged.

### Windows (src/assets/windows.rs: WindowSpec::new = ctor draws + BevelSharp, sample_asset, shells, window_mesh) - tool: tools/pywin.py, partcmp.py
- RNG draw order and all params: exact (compared against python f.params and bevel amount, every seed tried).
- Surface metric (python vs Rust, same vertex counts), 22 seeds at two sizes, no-curtain windows: mean 2e-8..1e-7, max <=1.3e-4 (worst: seed 9, shutter slats thinner than the bevel leg); bbox 1e-7.
- Curtain windows (~30%): geometry (cloth ribbons with sine wave, rod, 2 icospheres) ported but the Mesh Boolean (open meshes) is NOT emulated; measured mean 1.3e-5..1.8e-5, max 1.0e-3 (cloth narrow faces overlapped by the bevel), vertex count differs (boolean splits). bbox ~5e-7.
### Doors (src/assets/doors.rs, solid.rs): params exact for Panel/GlassPanel/Louver/Lite; door body/frame/handle-less assembly surface: mean <=5e-6, max mostly <=6e-5 (slab corner patches up to 5.5e-4, Blender triangulation). NOT done: handles (knob/lever/pull/bar geometry), loose hinge marker vertices (bbox), louver panels, dome arc, symmetric y copy, factory entry/openings integration.
### Chair: ChairParams + placeholder bbox verified exact (12 seeds, 3e-8 vs python). Asset geometry NOT ported (still the approximate seating.rs); measured vs python surface: mean 3-6 mm, max 2-10 cm (4 seeds). Needs fill_grid, solidify, subsurf, 8-segment round bevel and bridge_edge_loops(64 cuts) ports.

## Update: doors/windows integrated into scene generation (openings.rs)

Verified end-to-end against python: tools/opdump (bin opdump SEED out.json: rooms-only generation, dumps constants, cutters and the Rust pieces) -> tools/pyop.py (runs python room/decorate.py populate_doors / populate_windows on the same cutters with the same stage seeds int_hash((seed,'room_doors'|'room_windows'))) -> tools/cmpop.py (per-cutter surface metric of the union of pieces). Seeds 1-4 (22+17+21+19 cutters): factory choice, factory seeds, door/window instance seeds, door opening angles (rot_z), window sizes all match python exactly (RNG order incl. the hidden global draws: casing_factory.finalize_assets runs `surface.apply([])` after each door factory -> `apply_draws`).
- changed files: src/openings.rs (rewritten: make_openings(c, cutters, seed) - signature changed, gen2.rs updated), src/assets/{doors,windows,solid,doorhandles(new)}.rs, src/assets/mod.rs (+`pub mod doorhandles;`), src/bin/{gen2,dumppart,opdump(new),applytest(new)}.rs, tools/{pyop,cmpop,pyapply}.py. matdraw.rs: only the two additive `call()` arms (LampShade, FineKnitFabric).
- Windows (all 4 seeds, 70+ windows): non-curtain windows mean <=1e-6, max <=2.5e-4 (one 1.2e-4 / 5e-5 typical), bbox <=1e-6. Curtain windows (~30%): mean ~1.5e-5, max 1e-3..1.8e-2 on a few probes: the Mesh Boolean (cloth minus rod cylinder, open meshes) is not emulated and python's overlapped narrow faces differ slightly. WindowFactory sampling order verified (open flag drawn unless tall window, shutter/curtain drawn, then frame/rod/cloth material classes).
- Doors: slab with pockets + glass pockets (tag-split into glass piece), louver (through-hole + ring + blades) surface-exact (max <=3e-5), frames (single column / square / dome arc / double door), pieces named door{ci}, .glass, .louver, .handle with the factory's material classes (surface/glass/louver/handle|bar). Totals vs python (union of pieces incl. handle): bar-handle doors mean 3e-5 max 1.6e-3; pull/lever/knob doors mean 1e-4..3e-4, max 1e-3..1.4e-2 (handles are rebuilt from the same parameters/profiles but not with Blender's spin/subsurf/solidify meshing; the lever arm is the worst, ~1 cm). Handle placement is exact (hinge rule: child = child + parent_bbox_center + Position with u = uniform(.42,.45) drawn after the material applies; sliding bar paddle = child + parent bbox centre). Python adds 3 loose marker vertices per hinge (invisible, bbox only): not emitted. `door_arc` 8-segment bevel not emulated (0.001 chamfer instead).
- gen2 on 7 rooms (Bedroom/LivingRoom/Kitchen/DiningRoom/Bathroom): no panic, make_openings <130 ms.

## Update: seating family on the bpy-ops ports (bevelfull / bpyops / chair)

New files: src/assets/bevelfull.rs (faithful port of bmesh_bevel.cc as used by the BEVEL modifier: ANGLE limit, OFFSET or WIDTH, any segment count, profile 0.5, loop slide, clamp overlap, offset adjustment (least squares), weld/terminal/tri-corner/general ADJ vertex meshes via cubic_subdiv + interp_vmesh; pipes are rejected with Err), src/assets/bpyops.rs (fill_grid on a closed loop incl. the operator's corner/span selection, SOLIDIFY extrude mode with rim, angle-weighted vertex normals), src/assets/gnhelpers.rs (geo_radius evaluated by the gn interpreter from data/gn/geo_radius*.json), src/assets/chair.rs (ChairFactory), src/bin/{bevtest,chairparams}.rs, tools/{pybevel.py,bevcheck.sh,pyseatstages.py,seatcmp.sh,pychair.py,chpartcmp.sh,pybackstages.py,pyslats.py,pytowel.py}.
mod.rs diff: `pub mod bevelfull; pub mod bpyops; pub mod gnhelpers; pub mod chair;` and in make_factory `c if chair::handles(c) => chair::make(c, &mut rng, factory_seed),` before the seating line. blops.rs: additive `align_bezier_points` / `bezier_core` (draw.align_bezier handle edit).
Bevel port vs Blender BEVEL modifier on test solids (cube / sheared box / thin slab with clamp / bent square tube / rim ring, segments 1..8, OFFSET and WIDTH): mean <=2e-8, max <=1.9e-6 (tools/bevcheck.sh).
ChairFactory (exact parameters, exact placeholder, surface-exact asset): parts compared with python for seeds 1-18 (all leg types incl. up/down curved, round and square limbs, all four back types, arms): per-part mean <=1e-7 / max <=1e-6 (seat stage by stage: curve, fill_grid, solidify, subsurf, bevel all <=1.3e-7); whole asset (surface metric, vertex counts identical): mean 6e-9..2e-8, max <=5.5e-5 (seed 18, argmin tie), typically <=4e-6. Python quirks reproduced: select_loose+delete removes chain end vertices that continue into unselected edges; cut lines created by bridge_edge_loops give inward-wound boxes in decorate.solidify (bevel differs); one global bevel clamp for joined slats; fill_grid corner/span selection uses the edge-loop order of BM_mesh_edgeloops_find.
Asset build time ~1 s per chair (dominated by the 51k-vertex seat bevel / subsurf).

### Towel (src/assets/towel.rs, gnhelpers::geo_extension + data/gn/geo_extension.json, ctor_draws.rs TowelFactory arm now returns the 7 ctor values)
mod.rs: `pub mod towel;` and `c if towel::handles(c) && unseeded().is_some() => towel::make(c, &mut rng, factory_seed),`. Tools: pytowel.py, towelcmp.sh.
Fold towels (seeds 1-3): vertex counts equal, mean <=6e-9, max <=4e-7. ROLL towels (seeds 4,5,7): params/placeholder exact, preroll/roll/prebevel stages exact, but python's BEVEL on the zero-area strips (two rows mapped to one index) keeps more coincident duplicate vertices than my bevelfull (e.g. 47770 vs 47390 verts); final subsurf then differs: mean 3e-7..3e-4, max 1e-3..5e-3 -> APPROXIMATE.
### Status of the seating family
Chair: done (exact). Towel: fold exact, roll approximate. Sofa/ArmChair: GN check surface-equal, hand port surface-exact (index order not reproduced). OfficeChair/BarChair: hand-ported, GN checker still fails (unfixed). Bed (frame, mattress, pillow, assembly): NOT done; still approximate seating.rs.

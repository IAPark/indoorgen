# decor agent: DONE.md

Crate copy: ~/work/agents/decor/infinigen-rs. Owned: src/assets/{books,decor,plants,tableware,lamps,walldecor}.rs.

## Tools (new files)
- src/assets/: textdraw.rs (exact RNG replay of text.Text + Plaster), matdraw.rs (exact draw counts of ~20 material classes, validated by `matcheck` vs python seeds 0..39),
  gn.rs (float32 geometry-node primitives), blops.rs (Blender kd-tree remove_doubles, bpy spin, bezier_curve/curve2mesh, weld; validated), subsurf.rs (SUBSURF with limit-surface positions; validated vs Blender),
  can.rs, wineglass.rs, plate.rs, bottle.rs, cutlery.rs (Fork/Spoon/Knife), foodbox.rs.
- src/bin/: meshdiff.rs, phdiff.rs, matcheck.rs, subtest.rs.
- python: ~/work/agents/decor/tools/{dumpmesh.py, runcheck.sh, mkorc.py, dumpspin.py, matdraws.py, matnext.py, ...}.
- `tools/runcheck.sh module.Class N` dumps python spawn_asset(inst) and runs `meshdiff` (env VSET=1 TOPO=1): vertex count, max coordinate error (same index),
  vertex-set Hausdorff (order independent), polygon lists identical (exact), face-set equality after relabelling vertices.

## Shared-file diffs for the orchestrator (src/assets/mod.rs)
- `pub mod` lines: textdraw, matdraw, gn, can, blops, wineglass, plate, bottle, cutlery, foodbox, subsurf.
- make_factory: `books::make`, `plants::make`, `decor::make`, `lamps::make` now take `factory_seed` as 3rd arg (walldecor::make unchanged);
  new dispatch arms before tableware: foodbox, cutlery, bottle, plate, wineglass, can.

## Placeholders (phcheck on LR3_hs0b, LivingRoom_1, Bedroom_0, Kitchen_0): exact (<1e-5)
BookColumn (43+12+26), BookStack (25+35+30+6), DeskLamp (39+50+19+10), FloorLamp (4), PlantContainer (29+25+40+9), LargePlantContainer (9+17+39),
NatureShelfTrinkets (11+9+16), WallArt, Mirror, Can (4). FoodBox placeholder also exact.
NOT exact: JarFactory (1 entry, 5.5e-3; bmesh bridge/subsurf not ported).

## Mesh equality vs python (asset)
V = vertex count equal, err = max coord error with identical vertex order, P = polygon lists identical (exact), set = equal up to vertex relabelling.
| class | result |
|---|---|
| FoodBox | exact 4/4: err 0, P identical |
| Can | 6/6 V, err 9e-10, P identical |
| DeskLamp | 39/39 V, err 4.5e-7, P identical |
| FloorLamp | 4/4 V, err 1.2e-7, P identical |
| BookStack | 25/25 V, err 3e-8, P identical |
| BookColumn | 40/40 V, err 9.5e-7, P identical 38/40 (2 shortest-diagonal ties from f32 ulps) |
| Wineglass | 4/4 V, vertex order exact (err <=1.8e-7), faces equal as a set; face ORDER differs (BMesh mempool) |
| Plate | 3/3 set-equal: vertex Hausdorff <=8e-8, 0 face mismatches; order differs (Blender subsurf layout not reproduced) |
| Bottle | 4/4 set-equal: Hausdorff <=9.4e-7, 0 face mismatches |
| Spoon | 6/6 set-equal: Hausdorff <=1.2e-7 |
| Fork | 6/6 set-equal: Hausdorff <=7.5e-8 |
| Knife | 4/6 set-equal; 2/6 differ (make_knife_tip merge condition not fully reproduced) |
| BookFactory standalone | unseeded ctor draws; implemented from Rng::seeded(factory_seed); not oracle-checked |
| PlantContainer / LargePlantContainer / NatureShelfTrinkets | placeholders exact; asset meshes are APPROXIMATIONS (pot, plants, corals/rocks/shells/creatures not ported) |
| WallArt / Mirror | placeholders exact; asset approximate. Frame extrusion direction (+x or -x, ~17% -x) depends on a float-noise tie in bridge_edge_loops; rule not found; bevel 1/4 segments not ported |
| Bowl, Cup, Pot, Pan, Lid, Jar, Vase, Chopsticks, FoodBag, FruitContainer, Balloon, Blanket/Towel | NOT exact (old approximations): need BEVEL, bridge_edge_loops, booleans, noise textures, cloth sim, or the vase lofting graph |

## Key findings
- bmesh remove_doubles = BLI_kdtree_3d balance + calc_duplicates_fast in node-array order, first-come target (exact survivors); bpy spin phi = -(f32(2pi)/steps), iterative f32 rotation.
- draw.bezier_curve: reversed per-segment curve.subdivide with AUTO handle recalculation (5x length clamp), WELD clusters to centroid.
- SUBSURF vertices are limit-surface points; extrude regions move interior vertices in place, duplicate only boundary ones; solidify offset +1 flips original faces.
- Join Geometry orders inputs in reverse; geometry-node instances vanish when a modifier is applied (lamp bulb absent).
- Face/vertex order after spin/remove_doubles/subsurf is not reproducible without Blender's source; unordered equality is the achievable bar there.

## Open problems
Jar placeholder; Knife 2/6; WallArt/Mirror asset; Bowl/Cup (bevel); Pot/Pan/Lid; Vase; Chopsticks (join_objects transform quirk); plant and trinket meshes; face order for spin/subsurf classes.

# Phase 2 (Bowl, Cup, Vase, Lid, bevel)
## New files (all registered in mod.rs with `pub mod` + dispatch arms before the older ones)
vase.rs, bevel.rs, bowl.rs, cup.rs, lid.rs; books.rs gained `solidify_offset`, cutlery.rs `add_guard_x`/`grid` made pub. Bins: src/bin/rsmesh.rs. Tools: tools/cmp.py, tools/rc.sh `module.Class N`.
## Results (set = equal up to relabelling; order of subsurf/bevel output not reproduced)
| class | result |
|---|---|
| Vase | 4/4 vertex count equal (128514), Hausdorff <=1.5e-7, 0 face mismatches; pre-subsurf mesh polys identical (err 1e-7) |
| Bowl | 6/6 set-equal, Hausdorff <=2.6e-7, 0 face mismatches (bevel + subsurf) |
| Cup | 6/8 set-equal (Hausdorff <=6e-8); 2 with has_guard (handle boolean) NOT ported |
| Lid | 5/5 no-handle instances: vertex count equal, Hausdorff <=6.4e-8; 12-24 face mismatches per instance, all in the knob cylinder bevel (valence-3 verts: python emits degenerate quads with a repeated vertex, Rust emits 4-distinct quads); bar-handle instances (3) not ported |
## bevel.rs
Faithful port of the 2-beveled-edge vertex case (valence 4 and 3), quarter-ellipse profile, percent and absolute width, empirical global width clamp w <= L/(f(a,b)+f(b,a)) (two beveled ends) or L/f (one end), f = mean 1/sin(theta) towards the two beveled edges. Not covered: 3+ beveled edges at a vertex, valence >4 patches.
## Still not exact
Jar (placeholder 5.5e-3 off), Chopsticks, FoodBag, FruitContainer (plan: base Bowl/Pot asset, placeholder z>0.02 +0.05, z<0.02 -0.01, choice p=[.5,.5]), Balloon, Pot/Pan, Knife (2/6), WallArt/Mirror asset (bridge_edge_loops tie is a float-noise tie; bevel now available but frame not ported), PlantContainer/LargePlantContainer/NatureShelfTrinkets asset geometry, Cup handle, Lid handle. Plants/trees/leafy classes under assets/objects were not audited. Vertex/face ORDER after subsurf: layout understood ([coarse][3 per edge][face inner]) but coarse edge order from solidify not reproduced.
Debug env hooks (VASERAW, VRES, VASEFLIP) remain in vase.rs.

# Phase 3
Blender 4.2 sources (bmesh bridge/subdivide_edgering/beautify/bevel, editmesh_tools) were fetched into ~/work/agents/decor/bsrc (raw.githubusercontent.com works from the VM).
## New / changed (mod.rs: `pub mod bridge; pub mod jar;` + dispatch arms jar::handles, bowl::fruit_handles before bowl; matdraw.rs NOT edited this phase)
- bridge.rs: port of bridge_edge_loops for two closed loops (bridge_loop_pair flips/winding votes/expand/best rotation in float32, triangulate + beautify_fill (BLI_heap) for unequal loops, subdivide_edgering PATH interpolation with profile_shape_factor). The WallArt "+x / -x" tie is solved: it is the sign of dot(na+nb, el_dir) with el_dir ~ float noise of the loop centres; reproduced by float32 emulation of the plane transform, wire-solidify normals and BM_edgeloop_calc_center/normal.
- jar.rs: Jar 6/6 set-equal (n_base 4, 6, 64; Hausdorff <=5.3e-7, 0 face mismatches), every intermediate stage (simple subsurf, bridge+5 cuts, extrude, subsurf, solidify) checked. Bottom n-gon interior of the level-2 simple subdivision uses radii measured from Blender (n=6, 64) because OpenSubdiv evaluates the n-gon patches non-linearly. Placeholder follows from the exact asset bbox.
- FruitContainer (bowl.rs fruit_make): choice([Bowl, Pot]) draw, base asset, placeholder z shift; Bowl cases exact (3/6 seeds), Pot cases depend on the storage agent's Pot.
- WallArt/Mirror (walldecor.rs): ctor now draws bevel segments/width; frame built through bridge + solidify_offset: segments=0 instances exact (5/5, vertices 1e-9, face set equal, incl. the -x case). segments 1 and 4 (2/3 of seeds) are missing the BEVEL of 3-beveled-edge (box corner, convex and concave) vertices: frame is returned unbeveled (vertex count 20 vs 52/308). bevel.rs only handles 2-beveled-edge vertices; bmesh_bevel.cc is in bsrc (cube-corner vmesh).
- vase.rs: debug env hooks removed.
- tools/cmp.py: identity vertex mapping when index error <1e-6 (duplicate coincident vertices confused the nearest-vertex relabel).
## Still open
WallArt/Mirror bevel (3-edge vertices), Lid knob bevel degenerate quads (valence 3, 2 beveled + cap), Lid/Cup handles (boolean), Chopsticks, FoodBag, Balloon, Knife 2/6, PlantContainer/LargePlantContainer/NatureShelfTrinkets asset geometry, audit of plants/trees/leafy classes under assets/objects.

# Phase 4 (framework era; asset code stays in my own files)
mod.rs diff: `pub mod bevel3; pub mod chopsticks;` + arm `c if chopsticks::handles(c) => chopsticks::make(c, &mut rng, factory_seed)` (before jar). matdraw.rs / src/gn untouched.
| class | result |
|---|---|
| WallArtFactory, MirrorFactory | 12/12 + 12/12 seeds set-equal (Hausdorff <=8.3e-8, 0 face mismatches), all three bevel settings (0, 1, 4 segments) |
| ChopsticksFactory | 10/10 vertex counts equal, Hausdorff <=1.3e-7; 7 seeds face-set equal; the 3 seeds with a guard differ only in zero-area degenerate side faces (guard depth 0, coincident duplicate vertices) |
| KnifeFactory | 6/6 face-set equal (Hausdorff <=7.5e-8); make_knife_tip fixed: it is pointmerge over every edge with x>=0, |dx|<2e-4 and both y > y_offset*y_length (float32 vertex vs float64 threshold), including the row0-row1 edges of a column, then degenerate faces and the resulting wire edges / loose vertices vanish |
## New code
- bevel3.rs: BEVEL for vertices with three beveled edges (box corners; valence 3, or valence 4 with one coplanar unbeveled edge). Ported from bmesh_bevel.cc (sources in ~/work/agents/decor/bsrc): boundary verts (the meeting point of the two offset lines slid onto an unbeveled edge: |M| projected on the edge), profile from `isect_line_line` middle, cube-corner octant (`make_cube_corner_adj_vmesh`) only when every realised offset equals the width (tri_corner_test), otherwise the adj_vmesh (`cubic_subdiv`, fullness .551, sabin gamma) path; both verified. Convex and concave corners.
- chopsticks.rs: grid + solidify + write_co taper + subsurf(simple if square) + guard extrude (depth 0) + join_objects transform quirk (first object keeps local coordinates).
## Audit / still approximate (my classes)
Not exact: FoodBagFactory (needs bmesh edge-ring subdivide + WELD + extrude + musgrave geo_extension), BalloonFactory (text mesh + cloth sim; the placeholder needs generate_text + clip_gaussian), PlantContainerFactory / LargePlantContainerFactory (pot is the storage Pan/Pot; plants: cactus, mushroom, fern, succulent, spider/snake plant, monocot are separate large procedural ports), NatureShelfTrinketsFactory (corals, rocks, mollusks with texture DISPLACE, creatures; also stable-pose via trimesh), Cup/Lid handles (torus + SIMPLE_DEFORM taper + exact BOOLEAN against the subsurf'd body + split + subsurf: the BSP csg would not reproduce Blender's cut-face topology, so subsurf near the cut differs), Lid knob bevel (valence-3 with two beveled cap edges: 12-24 degenerate-quad mismatches).

# Phase 5 (plants through the GN framework)
mod.rs diff: `pub mod balloon; gnedit; snakeplant; spiderplant; succulent;` + arms `balloon, succulent, spiderplant, snakeplant` before chopsticks.
| class | result |
|---|---|
| SucculentFactory | 6/6 BIT-EXACT: vertex order and polygon lists identical, error <=2e-7 |
| SpiderPlantFactory | 6/6 face-set equal, Hausdorff <=6.5e-6 (vertex counts 100k-190k) |
| SnakePlantFactory | 6/6 face-set equal, Hausdorff <=3.7e-7 |
| BalloonFactory | placeholder exact (12/12 incl. generate_text length through clip_gaussian); asset approximated (cloth-inflated text) |
| PlantContainer / LargePlantContainer | succulent / spider / snake plants now exact meshes inside the container (plants.rs); cactus (voxel remesh + stucci displace + spikes), mushroom (REMESH), fern (1.6M vertices), monocot (stucci displace) remain approximate; the pot is the storage agent's (not exact) |
Method (reusable): `gnedit.rs` = JSON surgery on an exported group (set socket defaults / curve points, prune multi-input links to the sampled block count, find a block by upstream traversal), data/gn/*.json = groups exported once from the *largest* structure (python tools/gn_export.py asset ... then strip steps), a Rust function replays the python draws in order (get_params, then the node-builder draws in creation order). tools/drawlog.py logs the numpy draws of a factory; tools/plant_tpl.py (automatic affine fitting of constants to draws) was tried first and does NOT work for these factories because the draw program itself changes with the sampled counts.
## Framework fixes (src/gn etc.; each with a unit case in tools/gn_units.py)
- nodes_geo2.rs InstanceOnPoints: unlinked `Instance Index` is the implicit id-or-index field (was constant 0).
- nodes_mesh.rs SubdivisionSurface: also subdivides meshes inside instance references (modify_geometry_sets); `boundary_smooth` ALL smooths valence-2 boundary corners (subdiv.rs `set_corner_fixed`, default unchanged for the modifier).
- curveops.rs normals_min_twist rewritten after curve_poly.cc: normalized transport step, cyclic correction with angle_signed_on_axis (returns [0, 2pi)), applied to all points. (gn.rs lamp version untouched)
- curvekit.rs FloatCurve: the end-handle correction applies only to AUTO end handles (mixed AUTO + VECTOR curves were wrong). Unit `FloatCurve.mixed_auto_vector_end`.
- gncheck.rs: no panic when the Rust mesh has fewer faces than Blender's material_index list. gn_check.py: `GNTOL=` tolerance for --bisect, `GNALL=1` lists all failing probes.
- Known remaining: unit `ResampleAttr.align.23` (AlignEulerToVector of curve normals differs ~3e-6 in x/y euler: normal z noise), unit `VectorCurve` fails (pre-existing, not mine).

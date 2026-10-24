# Backlog

Acceptance for asset items: `refcmp <Class> 0-5` all PASS (surface mean <= 1e-5, max <= 1e-4; exact order is a bonus),
placeholders exact (`phcheck`), `tools/selfcheck.sh` green, Rust build of one instance well under 1 s.
Status values: todo / doing(<worker>) / done / blocked(<why>). Workers update their rows (numbers + next step) and commit.

## Seating / bedroom (worker: seat)
| id | item | status | notes |
|---|---|---|---|
| S1 | Bed: frame, mattress (coiled + wrapped), pillow base + Bed assembly using the Rust cloth (src/assets/cloth.rs, clothes.rs). Replace approximate seating.rs Bed | todo | furniture agent had started `bed.rs` (salvaged on master); Blanket/Comforter pre-sim exact; cloth sim approximate by design |
| S2 | Roll towels (TowelFactory roll type): bevel keeps fewer coincident duplicate vertices than Blender on zero-area strips (seed 4: 47390 vs 47770) | todo | pre-bevel mesh exact |
| S3 | OfficeChair/BarChair: gn_check failures | todo | hand port exists |
| S4 | Sofa/ArmChair exact vertex order (surface already exact) | todo | low priority |
| S5 | Chair performance: Rust 0.4-0.9 s vs python 0.13 s per chair | todo | profile; bevelfull? |

## Bathroom / kitchen (worker: bath)
| id | item | status | notes |
|---|---|---|---|
| K1 | Toilet mesh | todo | storage agent started `toiletx.rs` (uses bridge_ex SURFACE mode) |
| K2 | BathroomSink, StandingSink | todo | placeholders exact |
| K3 | Bathtub (alcove + freestanding) mesh | todo | bounds exact |
| K4 | Kitchen SinkFactory mesh | todo | bounds exact (ksink.rs) |
| K5 | TV / Monitor | todo | |
| K6 | Pan, Pot (+Lid) meshes | todo | bounds exact |
| K7 | KitchenSpace / KitchenIsland countertop slab + top cabinets | todo | |
| K8 | Oven gas variant (`use_gas=1`): exact boolean + bevel | todo | needs F2 |
| K9 | Microwave: Mesh Boolean (FLOAT solver) mismatch | todo | needs F2 |
| K10 | Aquarium contents (plants/corals) | todo | low priority |

## Decor / plants (worker: decor)
| id | item | status | notes |
|---|---|---|---|
| D1 | Monocot plant kind (LargePlantContainer is monocot-only) | todo | needs voxel remesh? legacy texture displace (legacynoise.rs started) |
| D2 | Fern | todo | |
| D3 | Cactus | todo | voxel REMESH |
| D4 | Mushroom | todo | |
| D5 | NatureShelfTrinkets: mollusks (~1/2 of draws), creatures (~1/3), corals, rocks | todo | mollusk.rs / nature.rs started; texture DISPLACE |
| D6 | FoodBag (WELD + bmesh extrude + musgrave geo_extension) | todo | |
| D7 | Cup / Lid handles (exact BOOLEAN) | todo | needs F2 |
| D8 | Lid knob bevel degenerate quads | todo | |

## Framework / pipeline (worker: frame)
| id | item | status | notes |
|---|---|---|---|
| F1 | Re-apply the framework agent's unmerged nodes_curves.rs changes (bezier handle nodes, FilletCurve fixes): `todo/framework_nodes_curves_unmerged.diff`; unit fails `curve2:SetHandle*`, `curve2:FilletCurve` | DONE: unit 745 pass / 5 fail (was 724/26) | master's nodes_curves.rs has storage's CDT FillCurve; keep it |
| F2 | Mesh Boolean exact (Blender EXACT and FLOAT solvers) without fragmentation (Spatula 36806 vs 2236 verts) | todo | blocks K8 K9 D7 |
| F3 | ConvexHull exact (~1.6e-4 now) | DONE| bullet_hull.rs (Bullet btConvexHullComputer port) already exact: ConvexHull unit cases pass vertex-exact. |
| F4 | Unify `bridge_ex.rs` (SURFACE mode) into `bridge.rs` | DONE| bridge_ex.rs removed; bridge.rs has bridge_pair_ex(.., surface) + bridge_two_loops_surface (toiletx uses it). Mechanical merge, builds; no cached asset exercises it yet. |
| F5 | Remaining unit failures: AlignEulerToVector, SampleNearest ties, DistributePointsOnFaces sphere, VectorCurve | done (partly)| unit 747 pass / 3 fail. AlignEulerToVector fixed (antiparallel projections give -angle); VectorCurve passes. Remaining: SampleNearest.FACE tie (BVH traversal order), DistributePointsOnFaces.sphere rot at one near-gimbal element (Blender normal differs from every f32 facet-normal formula by ~14 ulp, euler amplifies). Both ignorable. |
| P1 | Placement: Bathroom 0 diverges at stage 0 it 93: door vs bathtub MPR depth 0.567 vs python-fcl 0.482 for `collide(b,f)` (box-vs-box, support ties) | DONE| Cause: python-fcl (arm64) fuses a*b+c*d into FMAs in libccd (dot/cross/quat/pointTriDist); mpr.rs now emulates it (bit-exact depth/normal/pos vs fcl on the pair). Bathroom 0 identical through 730 iterations; added to selfcheck. |
| P2 | Placement: DiningRoom 0 diverges at stage 2 it 15 (wall_decorations term) | DONE| Same root cause as P1 (FMA in libccd); DiningRoom 0 identical through 770 iterations (add to selfcheck if wanted). |
| P3 | Remove oracles: Rust room mesh in Blender face order, placeholder meshes, populated assets, ctor draws (CTOREMU) so lockstep holds with no python dumps | todo | biggest remaining exactness item |

## Evaluation (orchestrator)
| id | item | status | notes |
|---|---|---|---|
| E1 | Spot-check renders, then blind A/B judging with >= 2 judges per room | todo | after S1, D1-D5 land |

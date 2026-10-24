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
| K11 | Scenes used the approximate `asset()` even where `exact()` existed. FIXED (orchestrator): `assets::scene_asset()` uses exact geometry + nearest-part materials, used by `RealAssets::visual/solver_asset` and refcmp (use `--raw` for plain asset()). KitchenCabinet, SingleCabinet, SimpleDesk, SidetableDesk, Hardware now PASS. Still failing: KitchenSpace/KitchenIsland/Sink/Monitor/TV/Pan/Pot | partly done | high value: kitchens are judged sparse/odd |

## Decor / plants (worker: decor)
| id | item | status | notes |
|---|---|---|---|
| D1 | Monocot plant kind (LargePlantContainer is monocot-only) | doing(decor) | `monocot.rs` + `leafkit.rs` + `dualcon.rs` (exact REMESH SHARP port, bit-equal) + `bpymod.rs` (SIMPLE_DEFORM/DISPLACE exact) + data/gn/monocot_flower.json. MonocotFactory vs python (surface mean): Agave s0 2e-6, Maize s1 3e-6 / s2 4e-6, Grasses s5 1e-6, Tussock s7 8e-6, Agave s3 4e-6; vertex counts within 0.01-0.1% (python 3.6M verts at face_size .003, too big for refcache; check with `tools/monocheck.sh SEED`, stage tracer `tools/pytrace.py` + `stagediff`). Open: Banana/Taro (np.argsort tie order of f32 lattice x depends on Blender's BM scanfill triangulation of the leaf outline -> need BLI_scanfill port; cut pattern differs), Wheat (needs voxel REMESH), Veratrum (spin loop+remesh_fill on non-planar), Agave cut_plane (cut_prob>0). Tools: tools/stagepick.sh, tools/pycontainer.py |
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
| P0 | **TOP PRIORITY (quality)**: Rust room mesh. Standalone `gen2 1 Kitchen` places 24 objects vs python 40; with only STATE0 (python room mesh) it places 41. Rust `shell::room_geom` emits only the visible interior surfaces (kitchen: 94 tris, sup 24.2 m2, wall 51.5 m2, no INTERIOR-tagged faces) while python's solidified room is a closed shell with thickness (224 tris: int 140.6/128, vis 105.7, sup 51.7, ceil 51.7, wall 124.6). Port python's room solidify (core/constraints/example_solver/room/solidifier.py + decorate split) so the Rust room mesh matches python's (same tags, ideally same face order: compare against STATE0 dumps with `ROOMSTAT=1 gen2 ...`). Then measure object counts standalone vs python on Kitchen 1, Bedroom 1, LivingRoom 1 | todo | evidence: env STATE0 alone restores the count |
| P3 | Remove oracles: Rust room mesh in Blender face order, placeholder meshes, populated assets, ctor draws (CTOREMU) so lockstep holds with no python dumps | todo | biggest remaining exactness item |

## refcmp sweep 2026-10-24 13:20 (master, `tools/refsweep.sh`; tests Factory::asset(), i.e. what scenes use)
- PASS all seeds: ArmChairFactory, BarChairFactory, BeverageFridgeFactory, BookColumnFactory, BookFactory, BookStackFactory, BottleFactory, BowlFactory, CanFactory, CellShelfFactory, ChairFactory, ChopsticksFactory, CoffeeTableFactory, DeskLampFactory, DishwasherFactory, FloorLampFactory, FoodBoxFactory, ForkFactory, JarFactory, LampFactory, LargeShelfFactory, MirrorFactory, OfficeChairFactory, PlateFactory, RugFactory, SideTableFactory, SimpleBookcaseFactory, SofaFactory, SpoonFactory, TVStandFactory, TableCocktailFactory, TableDiningFactory, VaseFactory, WallArtFactory
- FAILING: BalloonFactory 0/6 (mean 2.5e-1, max 2.2e0); BathroomSinkFactory 0/6 (mean 4.0e-2, max 1.7e-1); BathtubFactory 1/6 (mean 3.0e-2, max 1.8e-1); BedFactory 0/6 (mean 7.0e-1, max 3.7e3); BedFrameFactory 1/6 (mean 5.2e0, max 3.7e3); CeilingClassicLampFactory 0/6 (mean 0.0e0, max 0.0e0); CeilingLightFactory 0/6 (mean 1.5e-3, max 1.8e-2); CupFactory 3/6 (mean 3.7e-4, max 4.0e-2); FoodBagFactory 0/6 (mean 0.0e0, max 0.0e0); FruitContainerFactory 3/6 (mean 6.9e-3, max 2.6e-2); GlassPanelDoorFactory 0/3 (mean 0.0e0, max 0.0e0); HardwareFactory 0/6 (mean 1.4e-2, max 4.9e-2); KitchenCabinetFactory 0/6 (mean 1.3e0, max 2.9e0); KitchenIslandFactory 0/6 (mean 3.5e-2, max 3.0e-1); KitchenSpaceFactory 0/6 (mean 3.3e-2, max 3.1e-1); KnifeFactory 4/6 (mean 6.7e-3, max 4.1e-2); LargePlantContainerFactory 0/6 (mean 0.0e0, max 0.0e0); LiteDoorFactory 0/3 (mean 0.0e0, max 0.0e0); LouverDoorFactory 0/3 (mean 0.0e0, max 0.0e0); MattressFactory 0/6 (mean 9.2e-3, max 4.6e-2); MicrowaveFactory 0/6 (mean 8.9e-4, max 1.8e-1); MonitorFactory 0/6 (mean 3.3e-2, max 8.0e-2); MonocotFactory 0/1 (mean 0.0e0, max 0.0e0); NatureShelfTrinketsFactory 0/6 (mean 3.0e-2, max 6.5e-2); OvenFactory 5/6 (mean 4.2e-2, max 4.7e-1); PanFactory 0/6 (mean 5.4e-3, max 5.2e-2); PanelDoorFactory 0/3 (mean 0.0e0, max 0.0e0); PantsFactory 0/6 (mean 0.0e0, max 0.0e0); PillowFactory 0/6 (mean 9.4e-2, max 4.0e-1); PlantContainerFactory 0/6 (mean 2.8e-3, max 1.9e-2); PotFactory 0/6 (mean 6.9e-3, max 2.6e-2); RackFactory 0/6 (mean 0.0e0, max 0.0e0); ShirtFactory 0/6 (mean 0.0e0, max 0.0e0); SidetableDeskFactory 0/6 (mean 1.0e-4, max 2.8e-4); SimpleDeskFactory 0/6 (mean 1.0e-4, max 2.8e-4); SingleCabinetFactory 0/6 (mean 9.8e-3, max 8.3e-2); SinkFactory 0/6 (mean 1.1e-1, max 3.2e-1); StandingSinkFactory 0/6 (mean 4.6e-2, max 2.3e-1); TVFactory 0/6 (mean 3.2e-2, max 8.7e-2); ToiletFactory 4/6 (mean 6.0e-7, max 1.5e-3); TowelFactory 4/6 (mean 1.0e-5, max 5.0e-3); WindowFactory 0/6 (mean 0.0e0, max 0.0e0); WineglassFactory 0/6 (mean 1.4e-2, max 6.2e-2)
- Note: KitchenCabinet, SingleCabinet, SimpleDesk, SidetableDesk, Hardware have exact meshes only behind `Factory::exact()`; their `asset()` (used for scenes/renders) is still the old approximation -> K11.
- Window/doors have no make_factory entry (built in openings.rs), Pants/Shirt/Rack/CeilingClassicLamp/FoodBag/Monocot/LargePlantContainer(4.2M verts) not ported or not comparable.

## Evaluation (orchestrator)
| id | item | status | notes |
|---|---|---|---|
| E1 | First blind pass (2026-10-24, 11 rooms x cornerA, 3 sonnet judges, neutral prompt): **Rust preferred 8/33 = 24%** | done | losses: camera inside objects (garbled close-up, black near-clip triangle) -> render camera fixed for both sides; Rust beds are slabs (S1); dark Rust bathroom floors/lighting (Bathroom 4 lum 43 vs 87); sparse-looking Rust rooms (LivingRoom 4, Bedroom 4); 'floating' rugs (both sides) |
| E2 | Second pass after S1/D1-D5 + camera fix: re-render BOTH sides (camera code changed), 14 rooms x 2 views, 3 judges | todo | |

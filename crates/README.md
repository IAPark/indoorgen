# Workspace layout (split 2026-10-24 for fast incremental builds)

Layer order (each crate only depends on crates above it). Every crate re-exports its dependencies at its root, so old paths
like `crate::mesh::Mesh`, `crate::assets::polyfill`, `super::Asset`, `crate::furn::geom::ObjGeom`, `crate::gn::...` keep working.

| crate | contents (old location) |
|---|---|
| ig_core | dhash, pyhash, pyset, rng, bl32, shp, mesh, subdiv, materials, polyfill, int_hash, osd (OpenSubdiv FFI), vdb (OpenVDB FFI); build.rs + csrc/ (C/C++ shims) |
| ig_exbool | exact mesh boolean (src/exbool) |
| ig_bm | bmesh ports: bevel, bridge, hull, isect, qsort, bullet_hull, ... (src/bm) |
| ig_geom | solver geometry types: geom.rs (ObjGeom, Tri, Pose, Assets trait, PlaceholderKind), tags.rs (src/furn) |
| ig_kit | asset toolkit without the node interpreter: api (Asset, Part, Factory, scene_asset, inst_rng), cabnodes, gweld, curvekit, geomkit, blops, bpyops, bpyx, bevel*, bridge, solid*, matdraw, textdraw, ctor_draws (+unseeded), cloth, clothes, coil, tableparts, legtex, dualcon, prismcut, gnlite (old assets/gn.rs) ... |
| ig_gn | geometry-nodes interpreter (src/gn) |
| ig_furn | constraint solver (src/furn minus geom/tags) |
| ig_rooms | floorplan / room meshes (src/rooms) |
| ig_shared | asset helpers that need the interpreter: bathkit, subsurf, gnhelpers, gnedit, gnasset, hwexact, leafkit, voxelremesh |
| ig_seat | bed, chair, mattress, officechair, pillow, seating, sofa, stools, tables, towel |
| ig_storage | appliances, aquarium, bathroom, bathsink, bathtub, bathfree, bevelgn, cabdoor, cookware, gnports, ksink, pangeom, potbars, shelves, taps, toiletx, tvx |
| ig_decor | balloon, books, bottle, bowl, can, chopsticks, cup, cutlery, decor, foodbox, jar, lamps, lid, plate, proxy, tableware, vase, walldecor, wineglass |
| ig_plants | cactus, fern, mollusk, monocot, mushroom, nature, plants, shell, snakeplant, spiderplant, succulent |
| ig_openings | doors, doorhandles, windows |
| ig_assets | class registry (make_factory, RealAssets, memo caches) = old assets/mod.rs; re-exports every asset crate as `indoorgen::assets` |
| (root) infinigen | pipeline, openings (scene doors/windows), src/bin/* ; re-exports the crates under the historical paths |

Adding a module: put the file in the crate of its layer and add `pub mod x;` to that crate's src/lib.rs (asset classes also need a
dispatch arm in crates/ig_assets/src/lib.rs `make_factory`). A module that would need a crate below it to depend on a crate above it
belongs in a higher crate (or move the shared piece down). `cargo build --profile fast --bin <x>` from the repo root as before.

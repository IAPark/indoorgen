# DONE.md - Geometry Nodes framework (rng copy)

Deliverables: `tools/gn_export.py`, `tools/gn_check.py`, `tools/gn_units.py`, `tools/gn_survey.sh`, `tools/gn_table.py`, `src/gn/*` (interpreter),
`src/bin/gncheck.rs` (+`gnparams.rs`), `src/assets/gnasset.rs` + `gnports.rs` + `data/gn/triangle_shelf.json` (proof of reuse), `FRAMEWORK.md`.
Existing bins keep passing (phcheck LR3: all classes exact; the old assets/*.rs node code is untouched, `gn` reuses
curvekit::FloatCurve, gweld (merge/subdivide), cabnodes::gcube, polyfill, bl32, subdiv via adapters; it was NOT rewritten onto one mesh type).

## How to run
```
PY=~/work/infinigen-ref/.venv/bin/python; cargo build --release --bin gncheck
$PY tools/gn_check.py unit                       # 532 unit graphs vs Blender 4.2 (about 3 min); filter: unit "Math" "Voronoi"
$PY tools/gn_check.py group MODULE.FUNC --n 4 --bisect
$PY tools/gn_check.py asset MODULE.CLASS SEED --bisect     # every GN modifier applied while building the asset
$PY tools/gn_check.py factory shelves.triangle_shelf.TriangleShelfFactory SEED   # Rust Factory vs Blender final mesh
tools/gn_survey.sh LIST OUT.log                  # many factories sequentially
```

## Verification table (unit graphs; `tools/gn_table.py` on the final case file): 521 pass / 11 fail
Pass = exact vertex+face order and attributes (tol 1e-6 relative), or "surface-equal only" (same surface, different index order -
Mesh Boolean, Subdivision, Subdivide Mesh, Fill Curve, Split Edges, Ico/UV sphere variants, Extrude fill; marked in the third column).
Primitive edge ORDER is replicated only for Grid / Cylinder / Cone / CurveToMesh (60 passing cases differ in edge order; edges are compared as sets).

| node / family | passed | failed | of which surface-equal only | first failure |
|---|---|---|---|---|
| Math | 44 | 0 | 0 |  |
| VectorMath | 27 | 0 | 0 |  |
| CombineXYZ | 1 | 0 | 0 |  |
| SeparateXYZ | 3 | 0 | 0 |  |
| MapRange | 9 | 0 | 0 |  |
| Clamp | 2 | 0 | 0 |  |
| Compare | 16 | 0 | 0 |  |
| BooleanMath | 9 | 0 | 0 |  |
| Switch | 7 | 0 | 0 |  |
| RandomValue | 8 | 0 | 0 |  |
| FloatCurve | 5 | 0 | 0 |  |
| ColorRamp | 1 | 0 | 0 |  |
| Mix | 12 | 0 | 0 |  |
| InputPosition | 1 | 0 | 0 |  |
| InputIndex | 1 | 0 | 0 |  |
| InputNormal | 3 | 0 | 0 |  |
| pos | 3 | 0 | 0 |  |
| index | 2 | 0 | 0 |  |
| MeshCube | 4 | 0 | 0 |  |
| MeshCylinder | 9 | 0 | 0 |  |
| MeshCone | 7 | 0 | 0 |  |
| MeshUVSphere | 4 | 0 | 0 |  |
| MeshLine | 3 | 0 | 0 |  |
| MeshGrid | 2 | 0 | 0 |  |
| MeshCircle | 3 | 0 | 0 |  |
| CurveToMesh | 12 | 0 | 0 |  |
| QuadraticBezier | 3 | 0 | 0 |  |
| CurveQuad | 1 | 0 | 0 |  |
| CurveCircle | 1 | 0 | 0 |  |
| CurveLine | 1 | 0 | 0 |  |
| Transform | 9 | 0 | 0 |  |
| JoinGeometry | 2 | 0 | 0 |  |
| SetPosition | 2 | 0 | 0 |  |
| StoreNamedAttribute | 6 | 0 | 0 |  |
| CaptureAttribute | 1 | 0 | 0 |  |
| SetMaterial | 1 | 0 | 0 |  |
| SetMaterialIndex | 1 | 0 | 0 |  |
| BoundBox | 1 | 0 | 0 |  |
| InstanceOnPoints | 3 | 0 | 0 |  |
| RotateInstances | 2 | 0 | 0 |  |
| ScaleInstances | 2 | 0 | 0 |  |
| TranslateInstances | 2 | 0 | 0 |  |
| AttributeStatistic | 6 | 0 | 0 |  |
| SeparateGeometry | 6 | 0 | 0 |  |
| DeleteGeometry | 5 | 0 | 0 |  |
| SetShadeSmooth | 2 | 0 | 0 |  |
| FlipFaces | 2 | 0 | 0 |  |
| DomainSize | 1 | 0 | 0 |  |
| SplineParameter | 2 | 0 | 0 |  |
| SetCurveRadius | 1 | 0 | 0 |  |
| ResampleCurve | 9 | 0 | 0 |  |
| CurveToPoints | 3 | 0 | 0 |  |
| SampleCurve | 2 | 0 | 0 |  |
| MeshToCurve | 2 | 0 | 0 |  |
| CurveLength | 1 | 0 | 0 |  |
| FillCurve | 2 | 0 | 1 |  |
| SubdivideCurve | 1 | 0 | 0 |  |
| TrimCurve | 4 | 0 | 0 |  |
| FilletCurve | 31 | 0 | 0 |  |
| CurveArc | 1 | 0 | 0 |  |
| CurveSpiral | 1 | 0 | 0 |  |
| CurveBezierSegment | 1 | 0 | 0 |  |
| ReverseCurve | 1 | 0 | 0 |  |
| SplineType | 6 | 2 | 0 | SplineType.NURBS.bezier: attr fac [Point] elem 1: blender [0.027651779353618622] rust [0.0407653711736202 |
| CurveStar | 1 | 0 | 0 |  |
| SplineLength | 1 | 0 | 0 |  |
| CurveEndpointSelection | 1 | 0 | 0 |  |
| SampleIndex | 3 | 0 | 0 |  |
| NamedAttribute | 2 | 0 | 0 |  |
| GeometryNodeGroup | 1 | 0 | 0 |  |
| FieldAtIndex | 1 | 0 | 0 |  |
| FieldOnDomain | 1 | 0 | 0 |  |
| AccumulateField | 6 | 0 | 0 |  |
| MeshEdgeVertices | 4 | 0 | 0 |  |
| MeshFaceArea | 2 | 0 | 0 |  |
| MeshEdgeAngle | 2 | 0 | 0 |  |
| MeshEdgeNeighbors | 1 | 0 | 0 |  |
| MeshFaceNeighbors | 2 | 0 | 0 |  |
| MeshVertexNeighbors | 2 | 0 | 0 |  |
| FloatToInt | 4 | 0 | 0 |  |
| CombineSeparateColor | 3 | 0 | 0 |  |
| Proximity | 3 | 0 | 0 |  |
| SampleNearest | 2 | 1 | 0 | SampleNearest.FACE: attr i [Point] elem 3: blender [8.0] rust [6.0] |
| ObjectInfo | 2 | 0 | 0 |  |
| ShortestEdgePaths | 1 | 0 | 0 |  |
| EdgePathsToCurves | 1 | 0 | 0 |  |
| Raycast | 1 | 0 | 0 |  |
| SampleNearestSurface | 1 | 0 | 0 |  |
| Extrude | 13 | 1 | 1 | Extrude.fill.fillet: vertex 22 differs by 5.309e-1: blender [-0.09510565549135208, -0.280901700258255 |
| MeshBoolean | 6 | 0 | 6 |  |
| MergeByDistance | 3 | 0 | 0 |  |
| SubdivideMesh | 4 | 0 | 2 |  |
| SubdivisionSurface | 4 | 0 | 4 |  |
| Triangulate | 3 | 0 | 0 |  |
| ConvexHull | 0 | 2 | 0 | ConvexHull.grid: vertex 0 differs by 2.000e0: blender [1.850000023841858, 1.4500000476837158, -0. |
| SplitEdges | 2 | 0 | 2 |  |
| ScaleElements | 2 | 0 | 0 |  |
| DuplicateElements | 4 | 0 | 0 |  |
| NoiseTexture | 17 | 0 | 0 |  |
| AlignEulerToVector | 22 | 2 | 0 | AlignEulerToVector.Y.Z.0.0: attr r [Point] elem 8: blender [0.0, -0.0, -2.5132739543914795] rust [0.0, -0.0, |
| VectorRotate | 10 | 0 | 0 |  |
| RotateEuler | 4 | 0 | 0 |  |
| RotationNodes | 1 | 0 | 0 |  |
| MixRotation | 1 | 0 | 0 |  |
| GeometryToInstance | 1 | 0 | 0 |  |
| DistributePointsOnFaces | 4 | 2 | 0 | DistributePointsOnFaces.sphere.0: attr rot [Point] elem 2024: blender [-0.33817967772483826, 3.141592502593994, -0 |
| Points | 1 | 0 | 0 |  |
| IcoSphere | 3 | 0 | 2 |  |
| VectorCurve | 0 | 1 | 0 | VectorCurve: attr r [Point] elem 0: blender [-2.239100933074951, -1.4500000476837158, -1.4901 |
| Voronoi | 14 | 0 | 0 |  |
| WhiteNoise | 4 | 0 | 0 |  |
| Wave | 24 | 0 | 0 |  |
| **total** | 521 | 11 | | |

### The 11 failing unit cases (all understood)
* ConvexHull x2: Blender 4.2 moves hull vertices by ~1e-4 (shrinks toward the centroid) and merges quads differently; Rust keeps exact input points (max 1.6e-4 surface error). Treated as approximate.
* AlignEulerToVector x2: antiparallel vector (angle exactly pi), sign of a ~0 cross product.
* DistributePointsOnFaces sphere x2: euler gimbal +-pi representation of the same rotation (positions, ids, normals exact).
* SampleNearest.FACE: ties between equidistant faces resolved differently by Blender's BVH.
* Extrude.fill.fillet: Fill Curve triangulation (Blender: constrained Delaunay, Rust: ear clip + Lawson flips) differs for cocircular input.
* SplineType NURBS x2: positions exact (open: clamped knots, cyclic: periodic; count = resolution*segments); the Spline Parameter at NURBS control points differs.
* VectorCurve: extrapolation outside the clip rectangle differs (in-range values untested/unknown).

## Node coverage (top-45 list), verified against Blender 4.2 unless noted
Verified: Math, Reroute, CombineXYZ, SeparateXYZ, Transform (components), VectorMath, GroupInput/Output (+nested GeometryNodeGroup, fields through groups),
JoinGeometry (reverse link order), Value, MapRange (float+vector, 4 interpolations), Switch (field + geometry), SetPosition, NoiseTexture (1D-4D, fBm,
multifractal, ridged, hybrid, hetero, distortion), Voronoi 3D (F1, F2, smooth F1, distance to edge, n-sphere, 4 metrics), WhiteNoise, Wave, InputPosition, InputNormal,
InputIndex, InputID, StoreNamedAttribute, NamedAttribute, CaptureAttribute, SetMaterial/SetMaterialIndex, Compare (float/int/vector/color), BooleanMath, ColorRamp
(linear), Mix / MixRGB (common blend modes), MeshCube, MeshCylinder/Cone (fill rings, fans), MeshUVSphere, MeshLine, MeshGrid, MeshCircle, MeshIcoSphere,
CurveLine, CurveCircle (radius mode), CurveArc (radius mode), CurveSpiral, CurveStar, QuadraticBezier, BezierSegment, Quadrilateral (rectangle), CurveToMesh (caps, tilt,
radius, wire), BoundingBox, RealizeInstances, InstanceOnPoints, Rotate/Scale/TranslateInstances, GeometryToInstance, AttributeStatistic, FloatCurve, SplineParameter,
CurveTangent, RandomValue (all types), ResampleCurve, SetCurveRadius/Tilt, VectorRotate, AlignEulerToVector (auto + fixed pivot), RotateEuler, rotation nodes,
SeparateGeometry, DeleteGeometry (all modes), ExtrudeMesh (faces region/individual, edges, vertices), SetShadeSmooth, FlipFaces, MergeByDistance (ALL), MeshLine,
SubdivideMesh, SubdivisionSurface (no creases), FilletCurve (poly), MeshToCurve, SampleCurve, FillCurve, DuplicateElements, SampleIndex, Clamp, CurveToPoints, DeleteGeometry,
Triangulate, SplitEdges, ScaleElements (island semantics), Proximity, SampleNearest(Surface), Raycast, ShortestEdgePaths/EdgePathsToCurves, DomainSize, FieldAtIndex, AccumulateField,
FieldOnDomain, FloatToInt, Separate/CombineColor, TrimCurve, SubdivideCurve, ReverseCurve, CurveLength/SplineLength, mesh input fields (EdgeVertices, FaceArea, EdgeAngle, neighbors),
MeshBoolean (BSP, surface-equal), DistributePointsOnFaces (random mode, ids/normals/rotation), Catmull-Rom splines, NURBS positions, ObjectInfo (implemented, test added but not in the last full run).
Unverified / missing: Raycast (test added, last full run predates), ObjectInfo, ColorRamp non-linear modes, CollectionInfo, Dual Mesh, Poisson distribution, Voronoi 1D/2D/4D,
set handle type/positions & bezier auto handles, UV Map outputs of primitives (uv attributes are ignored by the checker; set GNUV=1 to compare), Mapping/TextureCoord (shader only), volume nodes, images.

## Real assets (tools/gn_check.py asset, seed 0; each GN modifier applied while building the asset is one "step")
Indoor survey (59 factories, 216 steps pass / 40 fail; 25 factories fully pass): TableDining, SideTable, CoffeeTable, CellShelf, TVStand, LargeShelf, SimpleBookcase,
SimpleDesk, TableTop, Chair (11 steps), Lamp/DeskLamp/FloorLamp, Basket, Plate, Can, Towel, ... Remaining differences (see FRAMEWORK.md gaps): Mesh Boolean fragment
counts (spatula 36806 vs 2236 vertices, surface not compared), Convex Hull shifts, Fill Curve CDT, a Delete-Geometry selection of the vase `Top` field, window group
(explosion in a boolean/duplicate chain), sofa/mattress (not bisected), UV attributes.
Bisect examples used to fix nodes: SideTable (anonymous attributes through CurveToMesh), Can (Scale Elements islands), triangle shelf (FilletCurve limit, merge edge attrs).

## Proof of reuse: TriangleShelfFactory (shelves/triangle_shelf.py)
`gn_export.py template shelves.triangle_shelf.TriangleShelfFactory 0 1 data/gn/triangle_shelf.json` -> 450 kB template, 12 parameter patches, 0 unmapped.
`src/assets/gnports.rs::triangle_shelf_params` replicates `get_asset_params` draw for draw (`inst_rng`; params identical to python for seed 3, checked with `gnparams`), registered in `make_factory`.
`gn_check.py factory`: seeds 0-6 surface-equal to Blender (<1e-5; max 1.9e-6); seed 7 differs by 2.7e-4 (profile fillet in `side_leg` for a rare parameter set; see open problems).
The same flow ports any asset whose geometry is one `add_geomod` call: template + sampler + one line in `handles`/`make`.

## Open problems / next steps
1. FilletCurve edge cases (cyclic, negative/zero radius counts, limit-radius rule when neighbouring segments differ) and the triangle-shelf seed-7 profile.
2. Mesh Boolean: output fragmentation (BSP); Blender merges coplanar pieces. A real exact-arithmetic or manifold-style boolean would shrink outputs and match vertices.
3. Convex Hull perturbation, Fill Curve CDT for cocircular input, Vase `Top` selection, NURBS spline parameter, VectorCurve extrapolation.
4. Primitive UV maps (Cube/Cylinder/Sphere/Grid), edge order of remaining primitives, CollectionInfo, Dual Mesh, Voronoi other dimensions, bezier auto handles.
5. Consolidation: `gn::Mesh` is a new type; GMesh/ExactMesh/Mesh(f64) are reached through adapters (`meshops::to_gmesh/from_gmesh`), not merged.

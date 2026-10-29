# tools/dataset: multi-view training data (RGB + metric depth + cameras) with independently varied factors

Purpose: fine-tune feed-forward multi-view reconstruction (VGGT family) under noisy conditions: doors open/closed, wall colours, mess, time of
day / lamps, furniture layout and people vary independently, the floor plan and the camera views stay shared.

```
BLENDER_PY=~/work/infinigen-ref/.venv52/bin/python          # bpy 5.2 + Pillow/numpy/OpenCV; the driver itself needs only the stdlib
$BLENDER_PY tools/dataset/make_dataset.py tools/dataset/examples/demo.json OUT/ [--resume] [--only HOUSE[/VARIANT]] [--keep-glb] [--dry-run]
$BLENDER_PY tools/dataset/validate.py OUT/<house>/<variant>        # consistency check (also run by the driver)
$BLENDER_PY tools/dataset/to_vggt.py OUT/                           # VGGT/Co3D-style extri/intri annotation files
```
gen2 is taken from `GEN2`, the dataset config `gen2`, `target/{fast,release}/gen2` or the master checkout's binary.

## Dataset config
```jsonc
{ "defaults": { "width": 960, "height": 720, "views": 4, "samples": 48, "engine": "cycles",   // or "preview"
                "depth_format": "npy",                    // npy | exr | both
                "extras": ["normal", "instance"],         // optional per-view normals / instance ids
                "gen": { "cloth": {"on": false} },        // partial GenConfig merged into every variant (src/config.rs)
                "cameras": { "focal_mm": [18, 35], "eye_height_m": [1.0, 1.8], "wall_margin_m": 0.45,
                             "traj_frac": 0.4, "cluster_len": 3, "pool_factor": 3 } },
  "houses": [ { "name": "bedroom5", "seed": 5, "room": "Bedroom", "camera_seed": 1, "views": 4,   // per-house overrides of the defaults
               "variants": [ { "name": "base", "vary": [] },                      // everything at the house seed's values
                             { "name": "light", "vary": ["lighting"], "count": 8 },      // 8 variants, only the lighting seed changes
                             { "name": "all", "vary": "all", "count": 5, "gen": {...} } ] } ] }
```
* factors: `doors walls mess lighting furniture people` (`vary: "all"` = all six). Every variant gets one seed per factor (`crc32(house seed, variant,
  factor)`, 0 when not varied) and a GenConfig override built from `factors.json` (`variation.<factor>.seed`, `mess.seed`, `people.seed`, plus the
  factor's mode so that the base and the varied variants draw from the same distribution). House seed, room and `only_target` are fixed per house.
* a gen2 binary without the `variation` section (before the factors worker merged) is detected from `gen2 --schema`: mess/people use the
  `fallback` entries (mess level, people count), `lighting` and `walls` are sampled by the driver (`variation.json` `_source` says so), `doors` and
  `furniture` cannot vary and are listed in `variation.json` `notes`.
* views: K per variant. The camera pool of a house (`OUT/<house>/cameras_pool.json`, 3 K candidates) is created by the first variant that is
  processed from the room's floor polygon and the camera seed, and reused by all other variants.

## Cameras (shared per house)
Positions are drawn inside the target room's floor (area weighted, >= 0.45 m from walls), eye height 1.0-1.8 m, roll 0, pitch -25..+12 deg, focal
18-35 mm (36 mm sensor, square pixels, principal point at the image centre). A cluster of up to `cluster_len` views starts with a fresh pose aimed at a
random room point; each following view is either a small trajectory step (0.3-0.7 m, yaw +-20 deg, similar focal) or a new position aimed at the
previous view's centre hit point (guaranteed overlap). Candidates are rejected when the eye is inside an object or <0.22 m from a surface, when the
frustum probe (9x7 rays) sees a median distance < 1.3 m, p10 < 0.35 m, > 20 % void or > 70 % bare wall/ceiling.
Per variant the pool is walked in order and the first K candidates that are still valid with that variant's furniture are used; views that
fail are dropped and replaced by the next pool entry, recorded in `cameras.json` (`pool_index` per view, `dropped_pool_views` with the reason).

## Output layout
```
OUT/index.jsonl                      one line per variant: path, house, variant, varied factors, seeds, views, size, time
OUT/<house>/cameras_pool.json        shared candidate poses (seed, positions, yaw/pitch, focal)
OUT/<house>/<variant>/
    images/NNN.png                   RGB 8 bit (Cycles with OIDN denoise, or the preview rasteriser)
    depth/NNN.npy                    float32 HxW metres, z-depth along the optical axis, 0 = no surface (miss / sky); valid mask = depth > 0
    normal/NNN.npy                   float16 HxWx3 world-space unit normals facing the camera (extras)
    instance/NNN.png + instances.json  uint16 object id per pixel (0 = none) -> {name, class, kind} (extras)
    cameras.json                     per view: K (3x3 px), w2c and c2w (4x4, OpenCV), width, height, near/far, focal_mm, pool_index, kind, valid_frac
    variation.json                   labels: every factor's seed, the effective lighting / wall colours, the GenConfig override, notes
    meta.json                        timings, sizes, engine, samples, dropped views, validation summary
    validation.json  sheet.png  gen2.log  render.log
```
Conventions: world = Blender/Infinigen world (z up, metres). Camera = OpenCV (x right, y down, z forward); `w2c` maps world to camera;
`K = [[f,0,W/2],[0,f,H/2],[0,0,1]]` with f = focal_mm / 36 * W; pixel (u, v) centres are at (u+0.5, v+0.5). Depth is the exact first-hit z-depth
from BVH ray casts through the pixel centres (anti-alias free, so no edge-mixed depths), taken on the same meshes Cycles renders (window glass
counts as a surface, blind slats leave holes = 0).

## VGGT conventions
VGGT's loaders (`training/data/datasets/*.py`, e.g. co3d.py) take per-sequence `extri` (OpenCV, world to camera) and `intri` (pixels), depth in
metres with 0 marking invalid pixels, images resized/cropped by the dataset class (aspect ratios are handled there). The same conventions are used
here; the per-dataset plumbing (file naming, annotation container) is the only difference: `to_vggt.py` writes a Co3D-like annotation list
(`filepath`, `depth`, `extri` 3x4, `intri` 3x3) per variant. Depth is `.npy` instead of 16-bit PNG (lossless, metres); `depth_format: "exr"` writes
OpenEXR. Differences to note: no per-pixel confidence mask file (use depth > 0), the images are not square-cropped.

## Object removal ("guess the depth of what is missing")
Per variant (or per variant group) `"remove": {...}`:
```jsonc
"remove": { "keys": ["82404_BedFactory"],                  // object keys (glb node "<key>#<part>", sidecar `objects`)
            "classes": ["Bed", "SofaFactory"],              // class names, case-insensitive substring of the key's class
            "random": { "count": 1, "classes": ["Shelf"], "seed": 3 },   // pick count objects (default: any furnishing > 25 cm)
            "with_children": true,                          // everything resting on a removed object goes too (sidecar `objects[].parent` chain)
            "views": "all"                                  // "all" | "random_subset" (+ "views_fraction" 0.5 | "views_count" n) | [view indices]
            "seed": 0 }                                     // view-subset seed (default derived from house/variant)
```
The removed objects (and their point lights / emissive lamp shaders) are hidden for Cycles in the selected views (one Blender import, visibility is
toggled per view; the cameras are identical for removed and untouched views).  Outputs per view when a variant has `remove`:
`depth/NNN.npy` = what the camera sees WITH the removal, `depth_full/NNN.npy` = depth of the full scene (the "what was there" target, equal
to `depth` outside the mask), `removed_mask/NNN.png` (255 = pixels covered by a removed object in the full scene; all 0 for views without removal),
and in `cameras.json` per view `removed_keys`, `removed_classes`, `removed_frac`, `depth_full`, `removed_mask`; `meta.json` has `dscap_removed_keys`
and `dscap_removed_views`, `variation.json` the resolved spec (`remove_spec`).  `normal/` and `instance/` describe the visible (removed) render.
The preview engine ignores `remove` (flagged in meta.json).

## Variation applied by the renderers
Cycles (`tools/render/render_rust.py --dataset SPEC`, module `dscap.py`): gen2's sidecar `variation` record (factors worker schema) ->
`lighting.sun_dir` (z-up unit vector toward the sun) -> sky texture sun rotation/elevation (Blender convention measured with a shadow test:
rotation r puts the sun toward (sin r, -cos r), i.e. compass bearing 180 deg - r), `sun_strength` -> sky sun intensity, `sky_strength` -> world
strength, `color_temp_k` -> multiply tint relative to 6500 K, point lights x `lamp_strength_scale`, per-lamp `lamps{key:{on}}` (off lamps: light absent
and emissive shader strength 0); `walls.colors` / `ceiling.colors` / `floor.colors` (linear rgb per room) -> base colour of that room's surface
materials (texture, bump and roughness kept). A gen2 without the record gets a driver-side sampler (`variation.json` `_source`). Preview engine:
`--sidecar-lighting` sun + glb base colours only (no GI, no procedural materials; the window pane does not write depth, so windows are holes = 0).

## Validation (`validate.py`)
Per ordered view pair: depth i is unprojected with K/c2w and projected into view j: (1) depth consistency |z - depth_j| (median relative error,
inlier fraction; the remainder is occlusion), (2) round-trip reprojection error in pixels, (3) photometric warp error of RGB against a 4 px shifted
control, (4) per view: depth-edge map vs RGB gradient shifted by -4..4 px: the best shift must be (0, 0), which proves the Cycles camera is the
camera described by K and c2w. Exit code 0 when every pair with >= 5 % overlap has reprojection < 1 px and depth error < 1 %, and all shifts are <= 1 px.

## Throughput (Cycles, 640x480, 16 spp + OIDN, 4 views, this 4-core VM while other workers run; demo bedroom seed 5)
Per variant: gen2 10-17 s (only_target, mesh budget on, 2 people, mess 0.4) + Blender import/materials/BVH ~26 s + 4 x ~16.5 s render (+1.5 s
ray-cast depth/normals/ids each) = ~105 s, 15 MB (20 MB with the removal outputs); validation <5 s. => ~135 views/h at K=4; K=16 per variant ~300 s
= ~190 views/h (the fixed ~40 s per variant amortises over K). Machine noise is large (the "walls" variant of the demo took 1197 s because
something else saturated the cores; "doors" 198 s). Suggested settings: 16 spp with denoise (denoised 16 spp is visually clean at 640x480; 64 spp
~3-4x the render time), K >= 8 views per variant, `extras` on (cheap), keep glbs off (`--keep-glb` only for debugging; a glb is ~50 MB).
Preview engine: ~12 s per variant (gen2 dominates, rasterising 6 views + depth ~1 s), RGB is flat-shaded (no procedural materials / GI).

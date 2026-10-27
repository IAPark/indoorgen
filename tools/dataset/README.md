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
{ "defaults": { "width": 640, "height": 480, "views": 4, "samples": 16, "engine": "cycles",   // or "preview"
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

## Variation applied by the renderers
Cycles (`tools/render/render_rust.py --dataset SPEC`, module `dscap.py`): `variation.lighting` -> sky texture sun rotation/elevation/intensity,
world strength, colour temperature (multiply tint relative to 6500 K), point lights x `lamp_strength_scale`, switched-off lamps / ceiling lights
(light removed and emissive shader strength 0); `variation.walls` (and ceiling/floor when recorded) -> base colour of that room's wall materials
(texture, bump and roughness kept). Preview engine: sun direction + glb base colours only (colours come from the glb; no GI, no lamps toggling).

## Validation (`validate.py`)
Per ordered view pair: depth i is unprojected with K/c2w and projected into view j: (1) depth consistency |z - depth_j| (median relative error,
inlier fraction; the remainder is occlusion), (2) round-trip reprojection error in pixels, (3) photometric warp error of RGB against a 4 px shifted
control, (4) per view: depth-edge map vs RGB gradient shifted by -4..4 px: the best shift must be (0, 0), which proves the Cycles camera is the
camera described by K and c2w. Exit code 0 when every pair with >= 5 % overlap has reprojection < 1 px and depth error < 1 %, and all shifts are <= 1 px.

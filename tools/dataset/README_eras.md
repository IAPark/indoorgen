# tools/dataset/make_eras.py: one home, several eras, photos + a target video

Training examples for "predict a video of a room from photos of the same home taken in other eras".

```
~/work/infinigen-ref/.venv52/bin/python tools/dataset/make_eras.py cfg.json OUT/ [--dry-run] [--only HOME] [--resume]
```
`cfg.json` overrides `DEFAULTS` at the top of the script, e.g. `{"homes": {"count": 100, "seed": 1, "rooms": ["LivingRoom", "Bedroom"]}}`.

## Eras
Era 0 sets the shell (floor plan, walls, doors, windows); every later era drifts from the one before. `levels` are chances (0..1) that a
factor changes from one era to the next:

| level | change |
|---|---|
| `furniture` | one of `furniture.kinds` (weights): `move` (pieces nudged), `rearrange` (rooms laid out again with new pieces, cheap), `replace` (a whole new solve, slow, 0 by default), `empty` (built-ins only) |
| `mess` | clutter level and seed (clutter only in the photographed room) |
| `people` | anyone there (1..`people.max`), placed in the photographed room |
| `repaint` / `refloor` | wall + ceiling finish / floor finish, drawn from the house style |
| `lighting` | time of day, sun, sky, lamps |
| `doors` | which doors stand open |
| `intrinsics` | focal length, aspect / orientation, principal point offset |

## Output per home
`manifest.json` (eras, what changed, people, finishes, lighting, timings), `shell.house.json`, `era_K/photos|depth|cameras.json`,
`target/frames|video.mp4|...`, `sheet.png`; `OUT/index.jsonl` over homes, `OUT/skipped.jsonl` for houses without the target room.
Cameras are OpenCV (x right, y down, z forward), `c2w`/`w2c` in metres (world z up), `K` in pixels and `K_normalized`.

## Reference photo choice (`ref_select`, `snapshots`, `micro_eras`)
`ref_select: "greedy"` (default): every candidate pose is rendered as plain raster depth at 1/4 size and turned into the room surfaces
it shows (15 cm cells); the same poses in the target era's scene give what changed; each next photo is the one adding the most unseen
target-video surfaces (seen from 2 directions 15 deg+ apart), room surfaces and changes (`ref_weights`). `snapshots`: gen2's camera pool
adds family-snapshot poses (in front of a person, within 50 deg of the way they face, 1.2-2.2 m, standing or sitting eye height,
24-35 mm, people 12-60 % of the frame); `snapshots.photos` (0.4) of an era's photos are reserved for them when it has people. Each era
writes selection.json; each target `ref_coverage`. Same 6 homes: old chooser video 78.3 % seen / 49.3 % from 2 directions / room 81.2 %
/ changes 80.4 %; greedy without snapshots 78.1 / 58.0 / 76.5 / 74.4; greedy with snapshots (default) 79.3 / 55.4 / 82.0 / 80.5.
`micro_eras.count` (default 0): after each era that many more with the same furniture, walls and floor and other mess and people
(`lighting: true` varies the light too), `micro_eras.photos` photos each. Measured 2026-11-08 (same 6 homes, one process, VM slower
that day than for the tables below: `rich` alone took 97.8 s a home): `rich` 12 examples in 601 s (72 per hour, 13.8 photos a home);
`rich` + `micro_eras.count` 1 + `budget_s` 150, 12 examples in 810 s (53 per hour, 25.8 photos a home). With `budget_s` 120 the
budget drops the second video in half the homes (45 per hour run alone). Two per era on living room 9105: 12 eras, 38 photos, 155 s.

### Where an era's time goes (and what is kept between eras)

The photo stills and depth checks go through one `preview --serve` process (`preview_serve`, default on): it keeps the last two
scenes loaded (the target era's is the one every era wants, so its depth is rendered first in each era), material pipelines,
simplified outdoor meshes and textures by content -- the same pictures as one process per call (depth identical; stills within the
path tracer's run-to-run noise). A gen2 batch keeps, besides the solve and the outdoors, the furnished layout (the solver's rooms
placed: `FURNISHED reused`) and each object's visual (`VISUALS n of m reused`), and the camera pool's ray casts use a BVH per node
kept between eras (`POOL world`): the built glb and pool are byte-identical to building each era from scratch (`ERA_MEMO=0` turns the
first two off). Per reused era alone: about 4-5 s of gen2 (was 6-8 s). In a running home the GPU is the limit (photos, depth checks
and the videos share it, and gen2 runs about 3x slower beside them); `timing.timeline` and `timing.photo_steps` in the manifest show
each era's turn on the main thread and its render steps. A/B on the 6 homes, interleaved: 97.8 s a home against 104.8 s before.

## Splat-grade target video (preset `splat`)
`"preset": "splat"`: one target era per home whose video is a 10 s survey walk (240 frames) around the room through the camera pool's
positions, looking inward with a sway, so a Gaussian splat can be trained on it. The walk is picked among a few variants by a splat
readiness score (eras_geom.splat_cells / splat_readiness: 15 cm surface cells inside the room seen from 3+ frames at 15 deg+ apart,
against what the camera pool sees); homes under `target.min_readiness` (0.7) fail the gate. Checked with Brush (Gaussian splatting,
5000 steps) on living room 9100, 12 held-out photos from other positions: survey 120 frames 25.4 dB, survey 360 frames 23.2 dB, the
old pan walk 120 frames 18.5 dB (same 4 views); the pan falls to 11-13 dB on views it never faced. Three homes: readiness 0.80-0.93,
57-100 s a home, 1 example each.

## Strategy (chosen by benchmark, tools/dataset/eras_bench.py)
Presets (`"preset": NAME` in the config): `per_home_60s` (every home inside 60 s: 3 eras, 2 x 64 frames, 60 s deadline),
`rich` (= the defaults: 4 eras, 2 x 120 frames, 120 s deadline), `volume` (4 eras, 3 x 64 frames). How they were chosen: a
coordinate search -- from the chosen setting each main knob (eras, videos per home, frames, photos per era, photo quality, deadline)
was changed one at a time on the same homes, and no single change did better within its limit. A local optimum on this VM, not an
exhaustive search.

Full grid under "every home inside 60 s" (eras {3, 4} x videos {2, 3 (4 eras only)} x frames {48, 64, 80}; 5 photos per era, full
photos, 60 s deadline; the same 6 homes for every configuration, each run cold; all 54 runs passed the quality gate):

| eras / videos / frames | examples per hour | mean / max s | homes over 60 s | refs per example |
|---|---|---|---|---|
| 3 / 2 / 48 | 121.7 | 49 / 55 | 0 | 7.0 |
| **3 / 2 / 64 (per_home_60s)** | 118.0 | 51 / 55 | 0 | 7.0 |
| 4 / 3 / 48 | 133.6 | 58 / 65 | 3 | 9.0 |
| 4 / 2 / 48 | 106.2 | 56 / 69 | 2 | 11.8 |
| 3 / 2 / 80 | 99.9 | 54 / 64 | 1 | 6.8 |
| 4 / 3 / 64 | 88.0 | 61 / 76 | 3 | 8.9 |
| 4 / 3 / 80 | 77.7 | 54 / 66 | 2 | 9.0 |
| 4 / 2 / 64 | 73.0 | 58 / 75 | 3 | 9.9 |
| 4 / 2 / 80 | 64.6 | 56 / 77 | 2 | 12.0 |

Only the 3-era, 48- and 64-frame configurations keep every home inside 60 s; they tie on examples per hour (within run noise), and
64 frames carry a third more video (about 7550 against 5840 training frames an hour), so per_home_60s is the best in this grid.

**Current default (target: at least 60 examples per hour, a home may take longer than 60 s):** 4 eras, two 120-frame (5 s) target
videos, `budget_s` 120. Same 6 homes: 87 examples per hour run alone (mean 82 s, max 101 s a home), 11.8 references per example,
82 % of each video shown in the references; six homes in one pipelined run: 12 examples in 407 s (106 per hour). For more volume:
4 eras, `target.count` 3, 64 frames (147 per hour, 8.8 references). The 60-s-per-home strategy below is `eras` 3, `target.frames` 64,
`budget_s` 60. Each era writes `selection.json`: every candidate pose, its estimated overlap with each walk, the overlap and spread
picks, what was rendered and dropped.

Defaults: 3 eras; **two target eras planned**, 64-frame videos; the second video is rendered only if the home still fits `budget_s`
(60), else that era stays a reference era; reference eras still being built ~9 s before the budget are left out; 5 photos per era
(2 per target era), one spare, a second render pass only for an era left short; full photo quality; video walk chosen on the CPU by
gen2 (`crates/ig_cams/src/pool.rs` video_paths: ~50 candidates from every pool view, clearance on every frame); camera pools of later
eras aim at what the walks see; photos picked partly for each walk in turn. gen2 `--batch` builds the eras of one furniture seed in one
process on the solved house; the target era is built first and its video renders in the background.

Measured on the 8 GB M2 VM (cold single homes unless noted):

| strategy | mean / max s | examples per hour |
|---|---|---|
| 1 video x 80 frames | 47.5 / 52.2 | 76 |
| 2 videos x 64, fast photos, no deadline | 60.4 / 72.7 | 119 (one home over 60 s) |
| 4 eras, 2 videos | 63.4 / 70.6 | 114 (over budget) |
| 1 video x 120 frames | 51.9 / 58.0 | 69 |
| full-quality photos | +3 s a home | -- |
| **default (2 planned, deadline)**, 13 cold homes | 52.7 / 59.4 | 91 |
| **default, 10 homes pipelined in one run** | 42.4 / 59.8 | 2 examples a home, 9 of 9 passed |

Clean comparison with the final code (10 new homes, each run cold and alone, strategies interleaved per home; 30 of 30 passed, none
skipped, none over 60 s):

| strategy | mean / max s | examples | per hour | refs per example | video seen in refs (median) |
|---|---|---|---|---|---|
| 2 videos x 64, deadline, fast photos | 49.1 / 56.4 | 17 | 124.6 | 6.8 | 0.71 |
| **2 videos x 64, deadline, full photos (default)** | 50.9 / 56.6 | 17 | 120.4 | 6.8 | 0.71 |
| 1 video x 80 | 44.2 / 59.0 | 10 | 81.5 | 9.1 | 0.80 |

Sweep around the default (6 of those homes, each strategy run cold; default rows are the same homes from the comparison above):

| strategy | mean / max s | over 60 s | examples | per hour | refs per example | frames per example |
|---|---|---|---|---|---|---|
| 2 videos x 48 frames | 49.3 / 54.8 | 0 | 10 | 121.7 | 7.0 | 48 |
| **default: 3 eras, 2 videos x 64, 5 photos, deadline** | 50.0-50.9 / 54.6-56.4 | 0 | 10 | 118-120 | 7.0 | 64 |
| 3 photos per era | 53.1 / 55.3 | 0 | 10 | 113.0 | 5.0 | 64 |
| 4 eras, 3 videos | 61.3 / 75.8 | 3 | 9 | 88.0 | 8.9 | 64 |
| 2 eras, 1 video | 41.1 / 48.6 | 0 | 6 | 87.7 | 4.8 | 64 |
| 1 video x 80 | 45.3 / 59.0 | 0 | 6 | 79.5 | 9.5 | 80 |
| 4 eras, 2 videos | 57.5 / 75.0 | 3 | 7 | 73.0 | 9.9 | 64 |

The default is the best of these that stays under 60 s: 48-frame clips give the same examples per hour (within noise) with 25 %
fewer frames each; fewer photos lower the references per example without saving time; 4 eras break the budget even with the deadline
(their extra generation runs in front of the deadline checks); fewer eras or one video give fewer examples.

On this VM generation slows 2-3x while the GPU renders; on a machine with more RAM and a separate GPU, `parallel_gen` rises and the
deadline keeps more second videos.

gen2 settings it uses (usable on their own): `house.pin_windows`, `people.rooms`, `mess.rooms`, `variation.furniture.rearrange`,
`variation.furniture.empty`, finish seeds from `variation.walls.seed` / `variation.floor.seed`; preview `--sidecar-lighting` for the RT sky.

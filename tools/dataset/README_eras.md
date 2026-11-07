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

## Strategy (chosen by benchmark, tools/dataset/eras_bench.py)
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

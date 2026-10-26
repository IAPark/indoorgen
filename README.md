# indoorgen

A Rust generator for furnished indoor scenes: floor plan and rooms, doors and windows, a constraint-based furniture
solver, about 90 procedural asset factories (furniture, appliances, decor, plants), optional lived-in clutter and people.
It is derived from [Infinigen Indoors](https://github.com/princeton-vl/infinigen) (Princeton University): the room
program, the solver semantics and the asset generators are ports of the Python originals, checked against the Python
code for matching meshes, placeholders and solver traces. **Blender is not used for generation**, only as an optional
renderer (Cycles) and as a test oracle.

Output: a glTF binary `scene.glb` plus a sidecar `scene.json` (node names, material specs, cameras, cloth jobs).
Generation is deterministic: the same arguments give byte-identical files.

## Features

- Rooms: floor plans, walls, ceilings, doors, windows (room types such as `LivingRoom`, `Kitchen`, `Bedroom`, `Bathroom`, `DiningRoom`).
- Furniture solver: Infinigen's constraint program (`data/home_program.json`) with simulated-annealing placement,
  reproducing the Python solver step for step (lockstep traces).
- About 90 asset factories (seating, tables, beds with cloth, storage, appliances, bathroom, tableware, lamps, plants, ...).
- `--mess LEVEL` (0..1): lived-in disorder (open doors and drawers, moved chairs, clutter).
- `--people N`: posed, textured [Microsoft Rocketbox](https://github.com/microsoft/Microsoft-Rocketbox) avatars.
- `--cams K`: photographer-like camera poses per room, written to the sidecar.
- Previews: a real-time wgpu/Metal viewer (`tools/preview.sh`), about 2 s per scene.
- Cycles rendering with Infinigen's own materials (needs Blender 5.2 and the Python Infinigen, see below).

## Quick start

Requirements:

- Rust (stable). A C/C++ compiler and `cmake`.
- **GEOS 3.11.x** (the `geos` crate is pinned to the 3.11 API). Newer system packages (for example Homebrew's) will not work.
  Either install 3.11 where `pkg-config` finds it, or build it:
  `cmake -S geos-3.11.4 -B build -DCMAKE_INSTALL_PREFIX=$HOME/geos-3.11.4 && cmake --build build && cmake --install build`
  and then set `PKG_CONFIG_PATH=$HOME/geos-3.11.4/lib/pkgconfig`, `GEOS_LIB_DIR=$HOME/geos-3.11.4/lib` and `GEOS_VERSION=3.11.4`.
  For a per-checkout setup copy `.cargo/config.toml.example` to `.cargo/config.toml` (gitignored) and edit the paths (it also sets the rpath).
- **assimp** (`brew install assimp` / `apt install libassimp-dev`), used to load avatar FBX files.
- **OpenSubdiv** 3.6.0 is vendored (`crates/ig_core/csrc/opensubdiv`) and built by `build.rs`; nothing to install.
- **OpenVDB** (optional; `brew install openvdb`): Blender's voxel remesh. Without it `build.rs` warns and uses a pure-Rust
  approximation, so some meshes (and file hashes) differ from the reference.

```sh
cargo build --release --bin indoorgen          # `gen2` is the same program under its historical name
target/release/indoorgen 3 LivingRoom out.glb  # seed, room type (optional), output
```

For the edit-compile loop use `--profile fast` (incremental, a little slower at run time).

## Commands

Generate (`indoorgen SEED [ROOM] [OUT.glb] [options]`):

```sh
indoorgen 1 Kitchen kitchen.glb
indoorgen 1 Kitchen kitchen.glb --mess 0.5 --people 2 --cams 3
```

Preview (builds on first use, about 1.5 min; Rust + wgpu, not PBR):

```sh
tools/preview.sh kitchen.glb --cams 6 --sheet sheet.png     # contact sheet of automatic cameras
tools/preview.sh kitchen.glb --view cam0 out.png            # a sidecar camera
```

People need the avatars: `tools/people/fetch_rocketbox.sh` downloads them (MIT licensed, about 450 MB) into
`$INDOORGEN_ASSETS` (default `<repo>/assets/rocketbox`; `assets/` is gitignored).

Render with Cycles and Infinigen materials (optional). Requires Blender 5.2's Python with Infinigen installed
(`pip install bpy==5.2.* && pip install --no-deps -e <infinigen checkout>`); `render_rust.py` patches the renamed sky model at run time:

```sh
export INDOORGEN_REF=/path/to/infinigen            # checkout containing .venv52 (or set BLENDER_PY directly)
$BLENDER_PY tools/render/render_rust.py kitchen.glb out.png --view cam0 --room Kitchen --seed 1 --samples 32
```

Scripts in `tools/render/` locate each other relative to their own path. `tools/people_view.sh`, `tools/cams_batch.sh` wrap them.

## Configuration (environment)

| variable | meaning | default |
| --- | --- | --- |
| `INDOORGEN_ASSETS` | Rocketbox avatars | `<repo>/assets/rocketbox` |
| `INDOORGEN_REF` | Python Infinigen checkout (reference/oracle, render materials), optional | `~/infinigen-ref` |
| `BLENDER_PY` | Python with bpy 5.2 + Infinigen, for `tools/render` | `$INDOORGEN_REF/.venv52/bin/python` |
| `PY_REF` | Python with bpy 4.2 + Infinigen, for the reference tools | `$INDOORGEN_REF/.venv/bin/python` |
| `INDOORGEN_REFCACHE` | cached Python reference meshes for `refcmp` | `<repo>/local-data/refcache` |
| `INDOORGEN_ORACLES` | Python oracle dumps (placeholders, solver traces) | `<repo>/local-data/ref` |

`tools/env.sh` resolves these for the shell tools and also reads `<repo>/.indoorgen.env` (gitignored) for per-machine values.

## Repository layout

- `crates/ig_*`: the workspace crates (see `crates/README.md`); the root package `indoorgen` holds the pipeline and the binaries in `src/bin`.
- `dev/`: one-off probes and comparison tools (separate package `indoorgen-dev`, not built by default).
- `data/`: the constraint program, asset tables and exported geometry-node graphs (about 10 MB).
- `tools/`: checks (`selfcheck.sh`, `opsuite.sh`, `refcmp`), preview, render scripts (`tools/render`), people tools.
- `WORKER.md`, `FRAMEWORK.md`, `BACKLOG.md`: development notes.

## Testing

`cargo test -p ig_core -p ig_exbool -p ig_gn -p ig_decor -p ig_mess -p ig_storage -p indoorgen` and `tools/ci/smoke.sh` need neither
Python nor Blender (this is what CI runs). `tools/selfcheck.sh` (lockstep placement against Python traces, placeholder oracles,
determinism) and `refcmp`/`opsuite.sh` need the Python reference and its cached dumps; they are local-only.

## Status and limitations

- Tested on macOS arm64. Linux is best-effort (CI job allowed to fail). Not tested on Windows.
- Furniture placement follows the Python solver step for step on the reference cases; asset meshes match the Python
  meshes at surface level for most factories, exact vertex order for many. Some assets are still approximations or use fallback bounding boxes.
- Random cameras (`--cams`) pass the quality filter in roughly 80% of cases.
- Materials are not evaluated in Rust: the sidecar carries material specs and Infinigen's Python materials are applied at
  render time (needs Blender 5.2 + the Python Infinigen). The previewer shows base colors only.
- Bed cloth is simulated by Blender at render time (the sidecar carries the cloth jobs).
- Wall-clock: roughly seconds to a minute per room depending on the room and profile.

## License and credits

GPL-3.0-or-later, see `LICENSE`. Parts are translations of Blender source (GPL-2.0-or-later) and of Infinigen
(BSD-3-Clause, Princeton University). Third-party libraries, data and the licensing audit are in `THIRD_PARTY_NOTICES.md`.
Rocketbox avatars are fetched, not redistributed. This project is not affiliated with Princeton, Blender or Microsoft.

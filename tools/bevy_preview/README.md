# bevy_preview

Headless Bevy 0.20 (wgpu/Metal) previewer for infinigen-rs `.glb` + sidecar `.json` scenes. Drop-in for
`tools/preview.sh` with the same command line (see `tools/bevy_preview.sh` for all options):

    tools/bevy_preview.sh SCENE.glb --at ex,ey,ez,tx,ty,tz[,lens] --res 1280x720 --sun 215,38 -o out.png
    tools/bevy_preview.sh SCENE.glb --cams 6 --sheet -o sheet.png       # auto cameras, scene loaded once
    tools/bevy_preview.sh SCENE.glb --cam cam.json --no-ceiling -o out_dir/

Rendering: Bevy PBR (glTF metallic/roughness, baseColor textures, alpha MASK leaf cards with clipped shadows,
coverage-preserving mips and edge dilation, double-sided), sun with 4 cascaded shadow maps to `--shadow-dist` (400 m),
sidecar point lights (W -> lm at 683 lm/W), SSAO, physical Atmosphere sky + sky-driven environment light (dimmed to
`--indoor-ambient` for cameras inside the house bbox), light distance fog, AgX tonemapping + saturation grade,
`--ss` supersampling (default 2x2), emissive, translucent `.glass`. The glb is parsed directly (no Bevy glTF loader) on a
thread while the render pipelines are compiled on a stand-in scene. Extra flags: `--dump-cams DIR` (write the resolved
cameras as cam.json, e.g. to render the same views with tools/preview.sh), `--tonemap`, `--saturation`, `--ambient`,
`--exposure` (0.5 = EV100 9.6), `--fog`, `--point-shadows N`, `-v` timings (`BP_FRAMES=1` per-frame timings).

Build: `cargo build --release` with `CARGO_TARGET_DIR=target` (standalone crate, not a workspace member; ~1.2 GB target).
First run after a shader change pays a one-off Metal shader compile (~10 s); later runs hit the OS shader cache.

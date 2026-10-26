# Licensing and third-party notices

This project is licensed under the **GNU General Public License v3.0 or later** (see `LICENSE`).

GPL is required because parts of the geometry code are Rust translations of Blender source code
(GPL-2.0-or-later): the exact mesh boolean (`crates/ig_exbool`), bmesh bevel and related operators (`crates/ig_bm`),
the dual-contouring remesher (`crates/ig_kit/src/dualcon*`), and other mesh operators and geometry-node semantics in
`crates/ig_kit` and `crates/ig_gn`. Blender: https://www.blender.org, copyright the Blender authors.

## Derived works and data

| Component | Used for | License |
| --- | --- | --- |
| [Infinigen](https://github.com/princeton-vl/infinigen) (Princeton University) | Scene-generation logic, asset generators, constraint program (`data/home_program.json`), exported node graphs (`data/gn/`), rendering helpers in `tools/render_scripts` | BSD 3-Clause, Copyright (c) 2023, Princeton University |
| [Blender](https://www.blender.org) | Translated mesh operators (see above); Blender is also the renderer at render time | GPL-2.0-or-later |
| [Microsoft Rocketbox](https://github.com/microsoft/Microsoft-Rocketbox) avatars | People (`crates/ig_people`); fetched by `tools/people/fetch_rocketbox.sh`, not stored in this repository | MIT |

Infinigen's BSD 3-Clause notice:

```
Copyright (c) 2023, Princeton University
Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
following conditions are met: (1) Redistributions of source code must retain the above copyright notice, this list of
conditions and the following disclaimer. (2) Redistributions in binary form must reproduce the above copyright notice,
this list of conditions and the following disclaimer in the documentation and/or other materials provided with the
distribution. (3) Neither the name of the copyright holder nor the names of its contributors may be used to endorse or
promote products derived from this software without specific prior written permission.
THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DAMAGES ARISING IN ANY WAY OUT OF
THE USE OF THIS SOFTWARE.
```
(Abridged; the full text is in Infinigen's `LICENSE`.)

## Linked or vendored libraries

| Library | How it is used | License |
| --- | --- | --- |
| [OpenSubdiv](https://github.com/PixarAnimationStudios/OpenSubdiv) 3.6.0 | Vendored subset in `crates/ig_core/csrc/opensubdiv` (see its `LICENSE.txt`) | Modified Apache 2.0 |
| [OpenVDB](https://www.openvdb.org) | Voxel remesh via `crates/ig_core/csrc/vdb_shim.cpp` | MPL-2.0 / Apache-2.0 (by version) |
| [GEOS](https://libgeos.org) 3.11.4 | 2D polygon operations, dynamically linked | LGPL-2.1 |
| [assimp](https://github.com/assimp/assimp) via `russimp` | Loading avatar FBX files | BSD 3-Clause |
| [rapier](https://rapier.rs) / [parry](https://parry.rs) | Physics settling, collision | Apache-2.0 |
| [wgpu](https://wgpu.rs) | Real-time previewer (`tools/preview`) | MIT / Apache-2.0 |

Other Rust crates are listed in `Cargo.lock`; their licenses are permissive (MIT/Apache-2.0/BSD) and compatible with GPL-3.0.

**To verify before release:** whether any exported node graph in `data/gn/` originates from Infinigen's GPL-3.0
submodule `infinigen_gpl` (compatible with this project's GPL-3.0, but it should be credited separately), and a
`cargo deny check licenses` pass over all dependencies.

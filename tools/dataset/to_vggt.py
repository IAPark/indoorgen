#!/usr/bin/env python3
"""to_vggt.py OUT_DIR [DEST]: write a VGGT/Co3D-style annotation next to every variant: DEST/<house>/<variant>.json (default OUT_DIR/vggt_annotations/)
= [{"filepath": "images/000.png", "depth": "depth/000.npy", "extri": 3x4 world->camera (OpenCV), "intri": 3x3 (pixels)}, ...]
VGGT's own loaders (training/data/datasets/*.py) read per-sequence extri/intri in OpenCV convention, depth in metres with 0 = invalid; only the file
plumbing differs per dataset, so a small dataset class pointing at these files is all that is needed."""
import os, sys, json
out = sys.argv[1]; dest = sys.argv[2] if len(sys.argv) > 2 else os.path.join(out, "vggt_annotations")
for line in open(os.path.join(out, "index.jsonl")):
    r = json.loads(line); vd = os.path.join(out, r["path"]); cams = json.load(open(os.path.join(vd, "cameras.json")))["views"]
    ann = [dict(filepath=v["file"], depth=v["depth"], extri=[row[:] for row in v["w2c"][:3]], intri=v["K"]) for v in cams]
    p = os.path.join(dest, r["house"], r["variant"] + ".json"); os.makedirs(os.path.dirname(p), exist_ok=True); json.dump(ann, open(p, "w"))
print("written to", dest)

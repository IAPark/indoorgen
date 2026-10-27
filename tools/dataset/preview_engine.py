"""preview engine of make_dataset.py: wgpu rasteriser (tools/preview, ~2 s per scene + 0.04 s per view) instead of Cycles.
RGB = flat base colours + sun + shadow map (no Infinigen materials, no GI); depth = the rasteriser's z-buffer (same pose/K conventions as the
Cycles engine, glass does not write depth).  The camera pool comes from the house's cameras_pool.json; when it does not exist yet, one
Blender process (pool_only, import + BVH only, no render) creates it, once per house."""
import os, sys, json, math, subprocess, time
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
BL2CV = np.diag([1.0, -1.0, -1.0, 1.0])


def cam_pose_cv(pos, yaw, pitch):
    fwd = np.array([math.cos(pitch) * math.cos(yaw), math.cos(pitch) * math.sin(yaw), math.sin(pitch)])
    right = np.cross(fwd, [0, 0, 1.0]); right /= np.linalg.norm(right)
    M = np.eye(4); M[:3, 0] = right; M[:3, 1] = np.cross(fwd, right); M[:3, 2] = fwd; M[:3, 3] = pos
    return M


def run_variant(a, vdir, glb, house, variant, variation, room, W, H, K, opt, out, hname):
    ref = os.environ.get("INDOORGEN_REF") or os.path.expanduser("~/work/infinigen-ref")
    py = os.environ.get("BLENDER_PY") or os.path.join(ref, ".venv52/bin/python")
    pool_path = os.path.join(out, hname, "cameras_pool.json"); res = {}
    if not os.path.exists(pool_path):
        spec = dict(out_dir=vdir, room=room, width=W, height=H, samples=1, n_views=K, camera_seed=opt("camera_seed", house["seed"]), cameras=opt("cameras", {}), pool_path=pool_path, pool_only=True, variation=variation, variant=variant["name"])
        sp = os.path.join(out, "_tmp", "pool_spec.json"); json.dump(spec, open(sp, "w")); t = time.time()
        with open(os.path.join(vdir, "pool.log"), "w") as f:
            subprocess.run([py, os.path.join(ROOT, "tools/render/render_rust.py"), glb, "unused.png", "--room", room, "--seed", str(house["seed"]), "--samples", "1", "--no-cloth", "--dataset", sp], stdout=f, stderr=subprocess.STDOUT, cwd=ref)
        res["pool_blender_s"] = time.time() - t
    pool = json.load(open(pool_path))["views"]
    for d in ("images", "depth", "_cams"): os.makedirs(os.path.join(vdir, d), exist_ok=True)
    cams = []
    for j, v in enumerate(pool):
        M = cam_pose_cv(np.array(v["pos"]), v["yaw"], v["pitch"])
        json.dump(dict(matrix=(M @ BL2CV).tolist(), lens=v["f_mm"], sensor_width=36.0), open(os.path.join(vdir, "_cams", "p%03d.json" % j), "w"))
    lg = variation.get("lighting", {}); sd = lg.get("sun_dir") or [0.5, -0.5, 0.7071]; pv_az = math.degrees(math.atan2(sd[1], sd[0])); pv_el = math.degrees(math.asin(max(-1.0, min(1.0, sd[2]))))
    cmd = [os.path.join(ROOT, "tools/preview.sh"), glb] + sum([["--cam", os.path.join(vdir, "_cams", "p%03d.json" % j)] for j in range(len(pool))], []) + \
          ["--res", "%dx%d" % (W, H), "--ss", "3", "--depth", ] + (["--sidecar-lighting"] if "lighting" in (json.load(open(glb[:-4] + ".json")).get("variation") or {}) else ["--sun", "%g,%g" % (pv_az, max(pv_el, 1.0))]) + ["-o", os.path.join(vdir, "_pv")]
    t = time.time(); r = subprocess.run(cmd, capture_output=True, text=True); res["preview_s"] = time.time() - t
    if r.returncode != 0: raise SystemExit("preview failed: " + r.stderr[-500:])
    stem = os.path.splitext(os.path.basename(glb))[0]
    views, dropped = [], []
    from PIL import Image
    for j, v in enumerate(pool):
        if len(views) >= K: break
        d = np.load(os.path.join(vdir, "_pv", "%s_p%03d.depth.npy" % (stem, j))); hit = d[d > 0]
        why = None
        if hit.size < 0.8 * d.size: why = "sees-void"
        elif np.percentile(hit, 10) < 0.35: why = "too-close"
        elif np.median(hit) < 1.3: why = "near-median"
        if why: dropped.append(dict(pool_index=j, reason=why)); continue
        k = len(views); nm = "%03d" % k
        os.replace(os.path.join(vdir, "_pv", "%s_p%03d.png" % (stem, j)), os.path.join(vdir, "images", nm + ".png"))
        np.save(os.path.join(vdir, "depth", nm + ".npy"), d.astype(np.float32))
        M = cam_pose_cv(np.array(v["pos"]), v["yaw"], v["pitch"]); fpx = v["f_mm"] / 36.0 * W
        views.append(dict(index=k, pool_index=j, kind=v["kind"], file="images/%s.png" % nm, depth="depth/%s.npy" % nm, width=W, height=H, K=[[fpx, 0, W / 2], [0, fpx, H / 2], [0, 0, 1]],
                          focal_mm=v["f_mm"], sensor_width_mm=36.0, w2c=np.linalg.inv(M).tolist(), c2w=M.tolist(), near=0.05, far=200.0, valid_frac=float((d > 0).mean()), depth_median=float(np.median(hit))))
    json.dump(dict(convention="opencv: x right, y down, z forward; w2c maps world->camera; world z up, metres; depth = z-depth along the optical axis, 0 = no surface", views=views, dropped_pool_views=dropped, engine="preview"),
              open(os.path.join(vdir, "cameras.json"), "w"), indent=1)
    import shutil; shutil.rmtree(os.path.join(vdir, "_pv"), ignore_errors=True); shutil.rmtree(os.path.join(vdir, "_cams"), ignore_errors=True)
    res["dscap_n_views"] = len(views); res["render_s"] = res.get("preview_s", 0) + res.get("pool_blender_s", 0)
    return res

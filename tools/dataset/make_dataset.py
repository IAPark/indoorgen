#!/usr/bin/env python3
"""make_dataset.py DATASET_CONFIG.json OUT_DIR [--keep-glb] [--only HOUSE[/VARIANT]] [--resume] [--dry-run] [--no-validate]

Builds a multi-view training set (VGGT-style: RGB + metric depth + OpenCV cameras) from generated houses with independently varied factors.
Run it with the Blender python (it only needs the stdlib itself):  $BLENDER_PY tools/dataset/make_dataset.py cfg.json out/
See tools/dataset/README.md for the config format and the output layout."""
import os, sys, json, time, zlib, shutil, subprocess, argparse, random, colorsys, copy, glob, math

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
FACTORS = ["doors", "walls", "mess", "lighting", "furniture", "people", "features"]
RENDER_FACTORS = ("lighting", "walls")  # applied by the renderer from variation.json when the sidecar carries none


def crc(*a): return zlib.crc32("/".join(map(str, a)).encode()) & 0x7FFFFFFF


def deep_merge(a, b):
    a = copy.deepcopy(a)
    for k, v in b.items():
        a[k] = deep_merge(a[k], v) if isinstance(v, dict) and isinstance(a.get(k), dict) else copy.deepcopy(v)
    return a


def subst(o, **kw):
    if isinstance(o, dict): return {k: subst(v, **kw) for k, v in o.items()}
    if isinstance(o, str) and o.startswith("{") and o.endswith("}"): return kw[o[1:-1]]
    return o


def find_gen2(cfg):
    for c in (cfg.get("gen2"), os.environ.get("GEN2"), os.path.join(ROOT, "target/fast/gen2"), os.path.join(ROOT, "target/release/gen2"), os.path.expanduser("~/work/infinigen-rs/target/fast/gen2")):
        if c and os.path.exists(c): return c
    sys.exit("gen2 binary not found: set GEN2 or dataset config 'gen2'")


def env_paths():
    ref = os.environ.get("INDOORGEN_REF") or os.path.expanduser("~/work/infinigen-ref")
    py = os.environ.get("BLENDER_PY") or os.path.join(ref, ".venv52/bin/python")
    return ref, py


# ---------------------------------------------------------------------------------------------- variants
def expand_variants(house):
    """[{name, vary: [factors], seeds: {factor: int}}] ; a group {"name","vary":[...]|"all","count":n}"""
    out = []; hs = house["seed"]
    for g in house["variants"]:
        vary = FACTORS if g.get("vary") == "all" else list(g.get("vary", []))
        bad = [f for f in vary if f not in FACTORS]
        if bad: sys.exit("unknown factor %s (known %s)" % (bad, FACTORS))
        n = int(g.get("count", 1)); base = g.get("name") or ("+".join(vary) if vary else "base")
        for i in range(n):
            name = base if n == 1 else "%s_%02d" % (base, i)
            seeds = {f: (crc(hs, name, f) if f in vary else 0) for f in FACTORS}
            out.append(dict(name=name, vary=vary, seeds=seeds, gen=g.get("gen", {}), index=i, remove=g.get("remove")))
    return out


def factor_config(registry, schema_has_variation, variant):
    """partial GenConfig for the factors of this variant (see factors.json). Returns (config, notes)"""
    cfg = {}; notes = []
    for f in FACTORS:
        r = registry.get(f, {}); s = variant["seeds"][f]; varied = f in variant["vary"]
        if schema_has_variation:
            parts = [r.get("always", {})] + ([r.get("seeded", {})] if varied else [])
        else:
            parts = [r.get("fallback", {})] if "fallback" in r else []
            if varied and not parts and f not in RENDER_FACTORS: notes.append("factor %s not supported by this gen2 (no variation section)" % f)
        for t in parts: cfg = deep_merge(cfg, subst(t, seed=s))
    return cfg, notes


# ---------------------------------------------------------------------------------------------- fallback render-side variation
def _lin(c): return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def fallback_variation(variant, room, sidecar_var):
    """effective render-side labels: sidecar `variation` (from gen2) wins; missing pieces are sampled here (labelled source=capture-fallback)
    from the factor seed (only when that factor is varied, else a fixed neutral default)"""
    v = copy.deepcopy(sidecar_var) if sidecar_var else {}
    src = dict(v.get("_source", {}))
    if "lighting" not in v:
        if "lighting" in variant["vary"]:
            r = random.Random(variant["seeds"]["lighting"]); ph = r.random()
            if ph < 0.65: lg = dict(sun_elevation_deg=r.uniform(15, 75), sun_strength=r.uniform(0.8, 1.2), sky_strength=r.uniform(0.18, 0.3), color_temp_K=r.uniform(5000, 7500), p_lamps=0.25, phase="day")
            elif ph < 0.85: lg = dict(sun_elevation_deg=r.uniform(-3, 8), sun_strength=r.uniform(0.5, 1.0), sky_strength=r.uniform(0.08, 0.15), color_temp_K=r.uniform(3500, 5000), p_lamps=0.7, phase="dusk")
            else: lg = dict(sun_elevation_deg=-25.0, sun_strength=0.0, sky_strength=0.01, color_temp_K=4000.0, p_lamps=1.0, phase="night")
            on = r.random() < lg.pop("p_lamps"); az = r.uniform(0, 360)
            lg.update(sun_azimuth_deg=az, sun_dir=[math.sin(math.radians(az)) * math.cos(math.radians(lg["sun_elevation_deg"])), math.cos(math.radians(az)) * math.cos(math.radians(lg["sun_elevation_deg"])), math.sin(math.radians(lg["sun_elevation_deg"]))], lamps_on=on, lamp_strength_scale=r.uniform(0.6, 1.4), ceiling_lights_on=on or r.random() < 0.3)
        else:
            lg = dict(sun_azimuth_deg=135.0, sun_elevation_deg=45.0, sun_dir=[0.5, -0.5, 0.7071], sun_strength=1.0, sky_strength=0.2, color_temp_K=6500.0, lamps_on=True, lamp_strength_scale=1.0, ceiling_lights_on=True, phase="default")
        v["lighting"] = lg; src["lighting"] = "capture-fallback" if "lighting" in variant["vary"] else "capture-default"
    if "walls" not in v and "walls" in variant["vary"] and room:
        r = random.Random(variant["seeds"]["walls"]); h, s, val = r.random(), r.uniform(0.05, 0.6), r.uniform(0.45, 0.95)
        v["walls"] = {room: [round(_lin(c), 4) for c in colorsys.hsv_to_rgb(h, s, val)]}; src["walls"] = "capture-fallback"
    v["_source"] = src
    return v


# ---------------------------------------------------------------------------------------------- helpers
def run(cmd, log, cwd=None, env=None):
    t = time.time()
    with open(log, "w") as f:
        p = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, cwd=cwd, env=env)
    return p.returncode, time.time() - t


def contact_sheet(vdir, K, py_png):
    import numpy as np
    from PIL import Image
    import cv2
    cams = json.load(open(os.path.join(vdir, "cameras.json")))["views"]
    rows = []
    for v in cams:
        im = np.asarray(Image.open(os.path.join(vdir, v["file"])).convert("RGB"))
        d = np.load(os.path.join(vdir, v["depth"])); dn = np.clip(d / 10.0, 0, 1); col = cv2.applyColorMap((dn * 255).astype(np.uint8), cv2.COLORMAP_TURBO)[:, :, ::-1]
        col[d <= 0] = 0
        col2 = []
        if v.get("depth_full"):
            mk = np.asarray(Image.open(os.path.join(vdir, v["removed_mask"])).convert("L")); ov = im.copy(); ov[mk > 127] = (0.4 * ov[mk > 127] + 0.6 * np.array([255, 0, 0])).astype(np.uint8)
            df = np.load(os.path.join(vdir, v["depth_full"])); c3 = cv2.applyColorMap((np.clip(df / 10.0, 0, 1) * 255).astype(np.uint8), cv2.COLORMAP_TURBO)[:, :, ::-1]; c3[df <= 0] = 0
            col2 = [ov, c3]
        rows.append(np.concatenate([im, col] + col2, 0))
    sheet = np.concatenate(rows, 1) if rows else np.zeros((8, 8, 3), np.uint8)
    h, w = sheet.shape[:2]; s = min(1.0, 1800.0 / w)
    Image.fromarray(sheet).resize((int(w * s), int(h * s))).save(py_png)


def dir_size(p): return sum(os.path.getsize(os.path.join(a, f)) for a, _, fs in os.walk(p) for f in fs)


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("config"); ap.add_argument("out")
    ap.add_argument("--keep-glb", action="store_true"); ap.add_argument("--only"); ap.add_argument("--resume", action="store_true")
    ap.add_argument("--dry-run", action="store_true"); ap.add_argument("--no-validate", action="store_true")
    a = ap.parse_args()
    cfg = json.load(open(a.config)); d = cfg.get("defaults", {}); out = os.path.abspath(a.out); os.makedirs(out, exist_ok=True)
    gen2 = find_gen2(cfg); ref, py = env_paths(); tmp = os.path.join(out, "_tmp"); os.makedirs(tmp, exist_ok=True)
    registry = json.load(open(os.path.join(HERE, "factors.json"))); registry.update(cfg.get("factors", {}))
    schema = subprocess.run([gen2, "--schema"], capture_output=True, text=True).stdout
    has_var = '"variation"' in schema
    print("gen2 %s  variation section: %s" % (gen2, has_var))
    idx_path = os.path.join(out, "index.jsonl")
    for house in cfg["houses"]:
        hname = house.get("name") or "%s_%d" % (house["room"].lower(), house["seed"])
        for variant in expand_variants(house):
            if a.only and a.only not in (hname, "%s/%s" % (hname, variant["name"])): continue
            vdir = os.path.join(out, hname, variant["name"])
            if a.resume and os.path.exists(os.path.join(vdir, "meta.json")): print("skip", vdir); continue
            def opt(k, dflt=None): return house.get(k, d.get(k, dflt))
            K, W, H = int(opt("views", 4)), int(opt("width", 960)), int(opt("height", 720))
            engine = opt("engine", "cycles"); samples = int(opt("samples", 48))
            fcfg, notes = factor_config(registry, has_var, variant)
            gcfg = deep_merge(deep_merge(d.get("gen", {}), house.get("gen", {})), variant.get("gen", {}))
            gcfg = deep_merge(gcfg, fcfg)
            gcfg = deep_merge(gcfg, {"house": {"seed": house["seed"], "room": house["room"], "only_target": house.get("only_target", d.get("only_target", False))}, "cameras": {"count": 1}})
            # people.count_choices: the house's people count drawn from the list (per house, the same for all its variants)
            pc = gcfg.get("people", {}).pop("count_choices", None)
            if pc: gcfg["people"]["count"] = int(pc[crc(house["seed"], "people_count") % len(pc)])
            print("== %s/%s vary=%s config=%s" % (hname, variant["name"], variant["vary"], json.dumps(gcfg)))
            if a.dry_run: continue
            os.makedirs(vdir, exist_ok=True)
            cfgp = os.path.join(tmp, "gen_cfg.json"); json.dump(gcfg, open(cfgp, "w"))
            glb = os.path.join(tmp, "%s_%s.glb" % (hname, variant["name"])); side = glb[:-4] + ".json"
            t_all = time.time()
            rc, t_gen = run([gen2, str(house["seed"]), house["room"], glb, "--config", cfgp], os.path.join(vdir, "gen2.log"))
            if rc != 0 or not os.path.exists(side): print("gen2 FAILED rc=%s, see %s/gen2.log" % (rc, vdir)); continue
            sidecar = json.load(open(side))
            slug = house["room"].lower().replace("room", "-room") if house["room"].endswith("Room") and house["room"] != "Room" else house["room"].lower()
            # gen2 writes cameras for every furnished room (room features furnish e.g. the living room too): take the target type's
            cams = sidecar.get("cameras") or []
            room = next((c.get("room") for c in cams if (c.get("room") or "").startswith(slug)), None) or (cams[0].get("room") if cams else None)
            if room is None:
                room = next((n["name"].rsplit(".", 1)[0] for n in sidecar["nodes"] if n["name"].startswith(slug) and n["name"].endswith(".floor")), None)
            variation = fallback_variation(variant, room, sidecar.get("variation"))
            rem = copy.deepcopy(variant.get("remove"))
            if rem:
                rem.setdefault("seed", crc(house["seed"], variant["name"], "remove"))
                if isinstance(rem.get("random"), dict): rem["random"].setdefault("seed", crc(house["seed"], variant["name"], "remove-objects"))
            variation.update(remove_spec=rem, variant_factors=variant["vary"], factor_seeds=variant["seeds"], gen_config=gcfg, notes=notes, room=room)
            json.dump(variation, open(os.path.join(vdir, "variation.json"), "w"), indent=1)
            meta = dict(house=hname, variant=variant["name"], house_seed=house["seed"], room_type=house["room"], room=room, engine=engine, width=W, height=H, views=K, samples=samples, gen_s=t_gen, glb_mb=os.path.getsize(glb) / 1e6)
            if engine == "cycles":
                spec = dict(out_dir=vdir, room=room, width=W, height=H, samples=samples, n_views=K, camera_seed=opt("camera_seed", house["seed"]), cameras=opt("cameras", {}),
                            pool_path=os.path.join(out, hname, "cameras_pool.json"), depth_format=opt("depth_format", "npy"), extras=opt("extras", ["normal", "instance"]),
                            variation=variation, variant=variant["name"], remove=rem)
                sp = os.path.join(tmp, "spec.json"); json.dump(spec, open(sp, "w"))
                rc, t_r = run([py, os.path.join(ROOT, "tools/render/render_rust.py"), glb, "unused.png", "--room", room, "--seed", str(house["seed"]), "--samples", str(samples), "--no-cloth", "--dataset", sp],
                              os.path.join(vdir, "render.log"), cwd=ref)
                res = None
                for line in open(os.path.join(vdir, "render.log")):
                    if line.startswith("DSRESULT"): res = json.loads(line[9:])
                if rc != 0 or res is None: print("render FAILED rc=%s, see %s/render.log" % (rc, vdir)); continue
                meta.update(render_s=t_r, **{"dscap_" + k: v for k, v in res.items()})
            else:
                from_preview = importlib_preview()
                if rem: print("WARNING: 'remove' is not supported by the preview engine (ignored for %s)" % variant["name"]); meta["remove_ignored_by_preview"] = True
                meta.update(from_preview(a, vdir, glb, house, variant, variation, room, W, H, K, opt, out, hname))
            if not a.keep_glb:
                for p in (glb, side): os.path.exists(p) and os.remove(p)
            if not a.no_validate:
                rc, t_v = run([py, os.path.join(HERE, "validate.py"), vdir, "--json", os.path.join(vdir, "validation.json")], os.path.join(vdir, "validate.log"))
                meta["validate_rc"] = rc; meta["validate"] = json.load(open(os.path.join(vdir, "validation.json")))["summary"]
            contact_sheet(vdir, K, os.path.join(vdir, "sheet.png"))
            meta.update(total_s=time.time() - t_all, size_mb=dir_size(vdir) / 1e6)
            json.dump(meta, open(os.path.join(vdir, "meta.json"), "w"), indent=1)
            with open(idx_path, "a") as f:
                f.write(json.dumps(dict(path=os.path.relpath(vdir, out), house=hname, variant=variant["name"], varied=variant["vary"], seeds=variant["seeds"], views=meta.get("dscap_n_views", K),
                                        width=W, height=H, engine=engine, total_s=meta["total_s"], size_mb=meta["size_mb"])) + "\n")
            print("done %s/%s total %.0fs (gen2 %.0fs render %.0fs) %.0f MB" % (hname, variant["name"], meta["total_s"], t_gen, meta.get("render_s", 0), meta["size_mb"]))
    shutil.rmtree(tmp, ignore_errors=True)


def importlib_preview():
    import importlib.util
    sp = importlib.util.spec_from_file_location("preview_engine", os.path.join(HERE, "preview_engine.py")); m = importlib.util.module_from_spec(sp); sp.loader.exec_module(m)
    return m.run_variant


if __name__ == "__main__":
    main()

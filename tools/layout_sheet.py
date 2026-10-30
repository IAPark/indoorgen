"""Top-down furniture layouts from `LAYOUT_DUMP=lN.json gen2 N ...` (solver footprints after the solve-level fixes, before meshes) and
2D layout metrics per room.
uv run --with matplotlib --with shapely tools/layout_sheet.py DIR  -> DIR/png/lN.png, DIR/sheet_*.png (4 x 3 per sheet), DIR/metrics.json

Metrics (walk radius 0.3 m = a 0.6 m path):
  path     the room's doors are not all connected by walkable floor
  reach    a floor piece (not rugs / chairs / lamps / plants) more than 0.45 m from the walkable floor connected to the doors
  door     the 0.8 m deep strip in front of a door (room side, from the wall face) is more than 25% covered by furniture
  window   furniture taller than the sill and 1 m within 0.25 m of a window (counter-run appliances exempt)
  floor pieces = unparented objects whose bottom is below 0.25 m, plus chairs / stools
  overlap  two floor pieces whose footprints overlap > 0.02 m2 at overlapping heights (chairs vs tables exempt)
  outside  a floor piece with > 0.02 m2 outside the room
  missing  the room's anchor piece is missing (bed / sofa / dining table / counter run / toilet + sink)"""
import glob, json, math, os, re, sys
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Polygon as MPoly
from shapely import wkt
from shapely.geometry import MultiPoint, Point, Polygon, box
from shapely.affinity import rotate, translate
from shapely.ops import unary_union
sys.path.insert(0, os.path.dirname(__file__))
from plan_sheet import draw as draw_plan, kind, footprint

CAT = [("bed", r"^Bed", "#7a9cc6"), ("seat", r"Sofa|ArmChair|^Chair|OfficeChair|BarChair|Stool", "#d08c5a"),
       ("table", r"Table|Desk", "#b5895a"), ("storage", r"Cabinet|Wardrobe|Shelf|Bookcase|TVStand|Dresser|Drawer", "#8f7f6f"),
       ("kitchen", r"KitchenSpace|KitchenIsland|Fridge|Oven|Dishwasher|Microwave|BeverageFridge", "#6f9f6f"),
       ("bath", r"Toilet|Bathtub|Shower|StandingSink|BathroomSink", "#5fb0b0"), ("screen", r"^TV|Monitor", "#444444"),
       ("lamp", r"FloorLamp|LargePlant|Plant", "#9cbf6a"), ("rug", r"Rug", "#e9d7c0"), ("keepout", r"Keepout", "#ff8080")]
ANCHOR = {"bedroom": [r"^Bed"], "living-room": [r"Sofa"], "dining-room": [r"TableDining|DiningTable"], "kitchen": [r"KitchenSpace"], "bathroom": [r"Toilet", r"StandingSink|BathroomSink"]}
NO_REACH = re.compile(r"Rug|^Chair|BarChair|Stool|FloorLamp|Plant|Keepout|SideTable|TV$|Monitor|Hardware")
ABBR = {"BedFactory": "bed", "SofaFactory": "sofa", "ArmChairFactory": "arm", "ChairFactory": "ch", "TableDiningFactory": "dining", "CoffeeTableFactory": "coffee",
        "SideTableFactory": "side", "SimpleDeskFactory": "desk", "WardrobeFactory": "wardrobe", "TVStandFactory": "tv", "KitchenSpaceFactory": "counter",
        "FridgeFactory": "fridge", "ToiletFactory": "wc", "BathtubFactory": "tub", "ShowerFactory": "shower", "StandingSinkFactory": "sink",
        "SingleCabinetFactory": "cab", "LargeShelfFactory": "shelf", "SimpleBookcaseFactory": "books", "CellShelfFactory": "cells", "KitchenCabinetFactory": "kcab",
        "DishwasherFactory": "dw", "OvenFactory": "oven"}

def cat(cls):
    for n, rx, col in CAT:
        if re.search(rx, cls): return n, col
    return "other", "#b0b0b0"

def foot(o, zmax=1.2):
    hs = [MultiPoint(p["xy"]).convex_hull for p in o["parts"] if p["z"][0] < zmax and len(p["xy"]) >= 3]
    hs = [h for h in hs if h.area > 1e-4]
    return unary_union(hs) if hs else None

def zr(o): return min(p["z"][0] for p in o["parts"]), max(p["z"][1] for p in o["parts"])

def analyse(d):
    t = d["wall_thickness"]
    rooms = {k: wkt.loads(v) for k, v in d["rooms"].items()}
    objs = [o for o in d["objects"]]
    floor = []  # (obj, footprint, z0, z1, room)
    for o in objs:
        z0, z1 = zr(o)
        # floor pieces: unparented objects standing on the floor, plus chairs (dining chairs are parented to their table); items
        # inside low cabinets / shelves also start below 0.25 m but have a furniture parent
        if z0 > 0.25 or re.search("Art|Mirror|Light|Window|Door", o["class"]): continue
        if o["parent"] is not None and not re.search("Chair|Stool", o["class"]): continue
        f = foot(o)
        if f is None: continue
        c = f.centroid
        room = next((k for k, p in rooms.items() if p.contains(c)), None)
        floor.append((o, f, z0, z1, room))
    out = {}
    for rk, rp in rooms.items():
        rt = kind(rk); inner = rp.buffer(-t / 2, join_style=2)
        items = [x for x in floor if x[4] == rk]
        block = unary_union([f for o, f, z0, z1, r in items if not re.search("Rug", o["class"])] or [Polygon()])
        free = inner.difference(block)
        walk = free.buffer(-0.3, join_style=2)
        comps = list(getattr(walk, "geoms", [walk])) if not walk.is_empty else []
        doors = []
        for ct in d["cutters"]:
            if ct["kind"] not in ("Door", "Entrance", "Passage") or rk not in ct["rooms"]: continue
            a = ct["rot"]; nx, ny = -math.sin(a), math.cos(a)
            for s in (1, -1):
                p = Point(ct["loc"][0] + s * nx * 0.55, ct["loc"][1] + s * ny * 0.55)
                if rp.contains(p): doors.append((ct, p, (s * nx, s * ny))); break
        fl = {}
        # doors connected: every door reaches the same walkable component
        hit = []
        for ct, p, _ in doors:
            ids = [i for i, c in enumerate(comps) if c.distance(p) < 0.35]
            hit.append(set(ids))
        conn = None
        if hit:
            common = set.intersection(*hit) if all(hit) else set()
            if not common: fl["path"] = "doors not connected" if len(hit) > 1 else "door blocked in"
            conn = unary_union([comps[i] for i in (common or set().union(*hit))]) if any(hit) else None
        # reach
        if conn is not None and not conn.is_empty:
            bad = [o["class"] for o, f, z0, z1, r in items if not NO_REACH.search(o["class"]) and f.distance(conn) > 0.45]
            if bad: fl["reach"] = bad
        # door strip
        for ct, p, (nx, ny) in doors:
            w = max(ct["dim"][0], ct["dim"][1]) if ct["kind"] != "Passage" else 0
            if w <= 0: continue
            strip = translate(rotate(box(-w / 2, t / 2, w / 2, t / 2 + 0.8), ct["rot"], origin=(0, 0), use_radians=True), ct["loc"][0], ct["loc"][1])
            if (strip.centroid.x - ct["loc"][0]) * nx + (strip.centroid.y - ct["loc"][1]) * ny < 0:
                strip = rotate(strip, 180, origin=(ct["loc"][0], ct["loc"][1]))
            cov = strip.intersection(block).area / max(strip.area, 1e-6)
            if cov > 0.25: fl["door"] = round(cov, 2)
        # windows
        for ct in d["cutters"]:
            if ct["kind"] != "Window" or rk not in ct["rooms"]: continue
            sill = ct["loc"][2] - ct["dim"][2] / 2
            zone = footprint(ct).buffer(0.25)
            # pieces up to 1 m (sofas, desks, sinks, beds) under a window are normal; counter-run appliances are part of the run
            bad = [o["class"] for o, f, z0, z1, r in items if z1 > max(sill, 1.0) + 0.02 and f.intersection(zone).area > 0.01 and not re.search("Rug|Curtain|KitchenSpace|Oven|Sink|Dishwasher|Plant|Lamp|^Bed", o["class"])]
            if bad: fl["window"] = sorted(set(bad))
        # overlaps
        ov = []
        for i in range(len(items)):
            for j in range(i + 1, len(items)):
                a, b = items[i], items[j]
                if re.search("Rug", a[0]["class"] + b[0]["class"]): continue
                if re.search("Chair", a[0]["class"] + b[0]["class"]) and re.search("Table|Desk", a[0]["class"] + b[0]["class"]): continue
                if a[3] < b[2] or b[3] < a[2]: continue
                if a[1].intersection(b[1]).area > 0.02: ov.append((a[0]["class"], b[0]["class"]))
        if ov: fl["overlap"] = ov
        outs = [o["class"] for o, f, z0, z1, r in items if f.difference(rp.buffer(0.01)).area > 0.02]
        if outs: fl["outside"] = outs
        miss = [rx for rx in ANCHOR.get(rt, []) if not any(re.search(rx, o["class"]) for o, *_ in items)]
        if miss: fl["missing"] = miss
        out[rk] = {"type": rt, "flags": fl, "walk": walk, "n": len(items)}
    return out, floor

def draw(d, ax, small=False):
    draw_plan(d, ax, small=small)
    res, floor = analyse(d)
    for o, f, z0, z1, r in sorted(floor, key=lambda x: (cat(x[0]["class"])[0] != "rug", x[3])):
        n, col = cat(o["class"])
        for g in getattr(f, "geoms", [f]):
            ax.add_patch(MPoly(list(g.exterior.coords), closed=True, fc=col, ec="#222", lw=0.4, alpha=0.55 if n == "rug" else 0.9, zorder=3 if n != "rug" else 2))
        if o.get("front") and n in ("bed", "seat", "storage", "kitchen", "bath", "screen") and f.area > 0.15:
            c = f.centroid; fx, fy = o["front"]
            ax.annotate("", xy=(c.x + fx * 0.35, c.y + fy * 0.35), xytext=(c.x, c.y), arrowprops=dict(arrowstyle="-|>", lw=0.5, color="#111"), zorder=5)
        lab = ABBR.get(o["class"])
        if lab and not small:
            c = f.centroid; ax.text(c.x, c.y - 0.12, lab, fontsize=5, ha="center", va="top", zorder=6, color="#111")
    # wall-mounted / raised pieces (no furniture parent, above 0.25 m): dashed outlines
    for o in d["objects"]:
        if re.search("Light|Lamp|Art|Mirror|Window|Door", o["class"]): continue
        if o["parent"] is not None and not re.search("Cabinet|Shelf|TV$", o["class"]): continue
        z0, z1 = zr(o)
        if z0 <= 0.25: continue
        f = foot(o, zmax=9)
        if f is None or f.area < 0.02: continue
        for g in getattr(f, "geoms", [f]):
            ax.add_patch(MPoly(list(g.exterior.coords), closed=True, fill=False, ec="#555", lw=0.5, ls="--", zorder=4))
    flags = []
    for rk, r in res.items():
        if not r["flags"]: continue
        flags.append(f"{r['type'].replace('-room','')}: " + ",".join(r["flags"].keys()))
        c = wkt.loads(d["rooms"][rk]).representative_point()
        ax.plot([c.x], [c.y + 0.5], marker="o", ms=4 if small else 6, color="#d62728", zorder=7)
    if flags:
        ax.text(0.0, -0.02, "; ".join(flags), transform=ax.transAxes, fontsize=5 if small else 7, color="#b22", va="top", wrap=True)
    return res

def main(dirn):
    files = sorted(glob.glob(os.path.join(dirn, "l*.json")), key=lambda f: int(re.findall(r"\d+", os.path.basename(f))[0]))
    files = [f for f in files if os.path.basename(f) != "metrics.json"]
    os.makedirs(os.path.join(dirn, "png"), exist_ok=True)
    plans = [json.load(open(f)) for f in files]
    metrics = {}
    for d in plans:
        fig, ax = plt.subplots(figsize=(8, 8)); res = draw(d, ax)
        fig.savefig(os.path.join(dirn, "png", f"l{d['seed']}.png"), dpi=150, bbox_inches="tight"); plt.close(fig)
        metrics[d["seed"]] = {k: {"type": v["type"], "flags": v["flags"], "n": v["n"]} for k, v in res.items()}
    per = 12
    for s in range(0, len(plans), per):
        fig, axs = plt.subplots(3, 4, figsize=(24, 19))
        for ax in axs.flat: ax.axis("off")
        for ax, d in zip(axs.flat, plans[s:s + per]): draw(d, ax, small=True)
        fig.tight_layout(); fig.savefig(os.path.join(dirn, f"sheet_{s // per + 1}.png"), dpi=100); plt.close(fig)
    json.dump(metrics, open(os.path.join(dirn, "metrics.json"), "w"), indent=1, default=str)
    # summary: share of rooms with each flag, per room type
    from collections import Counter, defaultdict
    tot = Counter(); fl = defaultdict(Counter); houses = Counter()
    for seed, rs in metrics.items():
        hf = set()
        for k, r in rs.items():
            tot[r["type"]] += 1
            for f in r["flags"]: fl[r["type"]][f] += 1; hf.add(f)
        for f in hf: houses[f] += 1
    keys = ["path", "reach", "door", "window", "overlap", "outside", "missing"]
    print(f"{len(plans)} houses; rooms with each flag (houses with it in any room on the last line)")
    print("%-12s %5s " % ("room", "n") + " ".join("%8s" % k for k in keys))
    for t in sorted(tot): print("%-12s %5d " % (t, tot[t]) + " ".join("%8d" % fl[t][k] for k in keys))
    print("%-12s %5d " % ("houses", len(plans)) + " ".join("%8d" % houses[k] for k in keys))

if __name__ == "__main__": main(sys.argv[1])

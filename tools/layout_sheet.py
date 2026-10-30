"""Top-down furniture layouts from `LAYOUT_DUMP=lN.json gen2 N ...` (solver footprints after the solve-level fixes, before meshes) and
2D layout metrics per room.
uv run --with matplotlib --with shapely tools/layout_sheet.py DIR  -> DIR/png/lN.png, DIR/sheet_*.png (4 x 3 per sheet), DIR/metrics.json

Metrics (walk radius 0.3 m = a 0.6 m path):
  path     the room's doors are not all connected by walkable floor
  reach    a floor piece (not rugs / chairs / lamps / plants / side tables) more than 0.45 m from the walkable floor reaching a door
           (chair-like pieces do not block reach: they are pulled out)
  door     the 0.8 m deep strip in front of a door (room side, from the wall face) is more than 25% covered by furniture
  window   furniture taller than the sill and 1 m within 0.25 m of a window (counter-run appliances exempt)
  floor pieces = unparented objects whose bottom is below 0.25 m, plus chairs / stools
  overlap  two floor pieces whose footprints overlap > 0.02 m2 at overlapping heights (chairs vs tables exempt)
  outside  a floor piece with > 0.02 m2 outside the room
  missing  the room's anchor piece is missing (bed / sofa / dining table / counter run / toilet + sink)"""
import glob, json, math, os, re, sys
# CLEAN=1: plans for review (no metric flags, no keep-out hatching, every piece labelled)
CLEAN = bool(os.environ.get("CLEAN"))
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
NO_REACH = re.compile(r"Rug|^Chair|OfficeChair|BarChair|Stool|FloorLamp|Plant|Keepout|SideTable|TV$|Monitor|Hardware")
ABBR = {"BedFactory": "bed", "SofaFactory": "sofa", "ArmChairFactory": "arm", "ChairFactory": "ch", "TableDiningFactory": "dining", "CoffeeTableFactory": "coffee",
        "SideTableFactory": "side", "SimpleDeskFactory": "desk", "WardrobeFactory": "wardrobe", "TVStandFactory": "tv", "KitchenSpaceFactory": "counter",
        "FridgeFactory": "fridge", "ToiletFactory": "wc", "BathtubFactory": "tub", "ShowerFactory": "shower", "StandingSinkFactory": "sink",
        "SingleCabinetFactory": "cab", "LargeShelfFactory": "shelf", "SimpleBookcaseFactory": "books", "CellShelfFactory": "cells", "KitchenCabinetFactory": "kcab",
        "DishwasherFactory": "dw", "OvenFactory": "oven", "SinkFactory": "sink"}

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
        if z0 > 0.25 or re.search("Art|Mirror|Light|Window|Door|Keepout|Hardware", o["class"]): continue
        if o["parent"] is not None and not re.search("Chair|Stool|SideTable", o["class"]): continue
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
        # reach: walkable floor without the chair-like pieces (pulled out to reach a table), components touching a door
        rblock = unary_union([f for o, f, z0, z1, r in items if not NO_REACH.search(o["class"])] or [Polygon()])
        walk2 = inner.difference(rblock).buffer(-0.3, join_style=2)
        comps2 = [c for c in (getattr(walk2, "geoms", [walk2]) if not walk2.is_empty else []) if any(c.distance(p) < 0.35 for _, p, _ in doors)]
        if comps2:
            conn2 = unary_union(comps2)
            bad = [o["class"] for o, f, z0, z1, r in items if not NO_REACH.search(o["class"]) and f.distance(conn2) > 0.45]
            if bad: fl["reach"] = bad
        # door strip
        for ct, p, (nx, ny) in doors:
            w = ct["dim"][0] if ct["kind"] != "Passage" else 0  # dim[0] = opening width along the wall, dim[1] = depth through it
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
                if re.search("SideTable", a[0]["class"] + b[0]["class"]) and re.search("^Bed", a[0]["class"] + b[0]["class"]): continue
                if a[3] < b[2] or b[3] < a[2]: continue
                if a[1].intersection(b[1]).area > 0.02: ov.append((a[0]["class"], b[0]["class"]))
        if ov: fl["overlap"] = ov
        outs = [o["class"] for o, f, z0, z1, r in items if f.difference(rp.buffer(0.01)).area > 0.02]
        if outs: fl["outside"] = outs
        miss = [rx for rx in ANCHOR.get(rt, []) if not any(re.search(rx, o["class"]) for o, *_ in items)]
        if miss: fl["missing"] = miss
        out[rk] = {"type": rt, "flags": fl, "walk": walk, "n": len(items)}
    return out, floor

def symbol(ax, o, f, fx, fy):
    """architectural plan symbols inside a footprint, in its own frame (u from the back toward the front, v across): pillows on a
    bed's head end, a sofa's backrest and arms, a toilet's tank and bowl, a tub's basin, burners on a range"""
    from matplotlib.patches import Ellipse, Circle
    sx, sy = -fy, fx
    pts = list(f.convex_hull.exterior.coords)
    us = [x * fx + y * fy for x, y in pts]; vs = [x * sx + y * sy for x, y in pts]
    u0, u1, v0, v1 = min(us), max(us), min(vs), max(vs)
    D, W = u1 - u0, v1 - v0
    P = lambda u, v: (fx * (u0 + u) + sx * (v0 + v), fy * (u0 + u) + sy * (v0 + v))
    box_ = lambda a, b, c, e, **k: ax.add_patch(MPoly([P(a, c), P(b, c), P(b, e), P(a, e)], closed=True, zorder=4.5, **k))
    ang = math.degrees(math.atan2(fy, fx))
    cl = o["class"]
    if cl == "BedFactory":
        n = 2 if W > 1.2 else 1
        pw = (W - 0.15 * (n + 1)) / n
        for i in range(n):
            a = 0.15 + i * (pw + 0.15)
            box_(0.08, 0.08 + min(0.4, D * 0.2), a, a + pw, fc="#e9eef7", ec="#333", lw=0.4)
        ax.plot(*zip(P(D * 0.36, 0.0), P(D * 0.36, W)), color="#333", lw=0.5, zorder=4.5)  # duvet fold
    elif cl in ("SofaFactory", "ArmChairFactory"):
        bk, arm = min(0.22, D * 0.25), min(0.18, W * 0.15)
        box_(0, bk, 0, W, fc="#a8683a", ec="#333", lw=0.4)
        box_(bk, D, 0, arm, fc="#a8683a", ec="#333", lw=0.4)
        box_(bk, D, W - arm, W, fc="#a8683a", ec="#333", lw=0.4)
        seats = max(1, round((W - 2 * arm) / 0.7)) if cl == "SofaFactory" else 1
        for i in range(1, seats):
            v = arm + (W - 2 * arm) * i / seats
            ax.plot(*zip(P(bk, v), P(D, v)), color="#333", lw=0.4, zorder=4.5)
    elif cl == "ToiletFactory":
        tk = min(0.2, D * 0.3)
        box_(0, tk, W * 0.05, W * 0.95, fc="#e8f4f4", ec="#333", lw=0.4)
        ax.add_patch(Ellipse(P(tk + (D - tk) * 0.5, W / 2), (D - tk) * 0.95, W * 0.8, angle=ang, fc="#e8f4f4", ec="#333", lw=0.4, zorder=4.5))
    elif cl == "BathtubFactory":
        ax.add_patch(Ellipse(P(D / 2, W / 2), D * 0.8, W * 0.86, angle=ang, fc="#d6eff2", ec="#333", lw=0.4, zorder=4.5))
    elif cl == "StandingSinkFactory":
        ax.add_patch(Ellipse(P(D * 0.55, W / 2), D * 0.6, W * 0.7, angle=ang, fc="#e8f4f4", ec="#333", lw=0.4, zorder=4.5))
    elif cl == "OvenFactory":
        for a in (0.3, 0.7):
            for b in (0.3, 0.7):
                ax.add_patch(Circle(P(D * a, W * b), min(D, W) * 0.13, fill=False, ec="#222", lw=0.5, zorder=4.5))


def draw(d, ax, small=False):
    draw_plan(d, ax, small=small)
    res, floor = analyse(d)
    for o in d["objects"]:  # door clearance / hearth keep-outs: hatched, not furniture (hidden in CLEAN mode)
        if o["class"] != "Keepout" or CLEAN: continue
        f = foot(o, zmax=9)
        for g in getattr(f, "geoms", [f]) if f is not None else []:
            ax.add_patch(MPoly(list(g.exterior.coords), closed=True, fill=False, ec="#e07070", lw=0.4, hatch="////", zorder=2))
    for o, f, z0, z1, r in sorted(floor, key=lambda x: (cat(x[0]["class"])[0] != "rug", x[3])):
        n, col = cat(o["class"])
        for g in getattr(f, "geoms", [f]):
            ax.add_patch(MPoly(list(g.exterior.coords), closed=True, fc=col, ec="#222", lw=0.4, alpha=0.55 if n == "rug" else 0.9, zorder=3 if n != "rug" else 2))
        if o.get("front") and (n in ("bed", "seat", "storage", "kitchen", "bath", "screen") or re.search("Desk|TVStand", o["class"])) and f.area > 0.04:
            fx, fy = o["front"]
            if CLEAN:
                # the back edge (a sofa's backrest, a bed's headboard, the side against the wall) as a thick dark band
                hull = f.convex_hull.exterior.coords
                best = None
                for (x1, y1), (x2, y2) in zip(hull[:-1], hull[1:]):
                    L = math.hypot(x2 - x1, y2 - y1)
                    if L < 1e-6: continue
                    mx, my = (x1 + x2) / 2 - f.centroid.x, (y1 + y2) / 2 - f.centroid.y
                    sc = -(mx * fx + my * fy) + L * 0.01   # the back edge (opposite the front)
                    if best is None or sc > best[0]: best = (sc, (x1, x2), (y1, y2))
                if best: ax.plot(best[1], best[2], color="#111", lw=2.2, solid_capstyle="butt", zorder=5)
                symbol(ax, o, f, fx, fy)
                # pieces that face something (seats, beds, TV stands, desks): a pointer outside the front edge, the way they face
                if re.search("Sofa|Chair|^Bed|TVStand|Desk|Toilet", o["class"]):
                    hx = [(x, y) for x, y in hull]
                    fe = max(((x1 + x2) / 2 * fx + (y1 + y2) / 2 * fy, (x1, y1, x2, y2)) for (x1, y1), (x2, y2) in zip(hx[:-1], hx[1:]))[1]
                    mx, my = (fe[0] + fe[2]) / 2, (fe[1] + fe[3]) / 2
                    L = 0.22 if f.area > 0.3 else 0.14
                    tri = [(mx - fy * L * 0.6, my + fx * L * 0.6), (mx + fy * L * 0.6, my - fx * L * 0.6), (mx + fx * L, my + fy * L)]
                    ax.add_patch(MPoly(tri, closed=True, fc="#111", ec="none", zorder=6))
            else:
                c = f.centroid
                ax.annotate("", xy=(c.x + fx * 0.35, c.y + fy * 0.35), xytext=(c.x, c.y), arrowprops=dict(arrowstyle="-|>", lw=0.5, color="#111"), zorder=5)
        lab = ABBR.get(o["class"]) or (re.sub(r"([a-z])([A-Z])", r"\1 \2", o["class"].replace("Factory", "")).split(" ")[-1].lower() if CLEAN else None)
        if CLEAN and n == "rug": lab = None
        if lab and (not small or CLEAN):
            c = f.centroid; ax.text(c.x, c.y - 0.12, lab, fontsize=5, ha="center", va="top", zorder=6, color="#111")
    # wall-mounted / raised pieces (no furniture parent, above 0.25 m): dashed outlines
    for o in d["objects"]:
        if re.search("Light|Lamp|Art|Mirror|Window|Door|Trinket|Book|Hardware", o["class"]): continue
        if o["parent"] is not None and not re.search("Cabinet|Shelf|TV$|^Sink|^Oven|Cooktop", o["class"]): continue
        z0, z1 = zr(o)
        if z0 <= 0.25: continue
        f = foot(o, zmax=9)
        if f is None or f.area < 0.02: continue
        is_sink = o["class"] == "SinkFactory"
        for g in getattr(f, "geoms", [f]):
            if is_sink and CLEAN: ax.add_patch(MPoly(list(g.exterior.coords), closed=True, fc="#a9d8f2", ec="#2a6f93", lw=0.6, zorder=4))
            else: ax.add_patch(MPoly(list(g.exterior.coords), closed=True, fill=False, ec="#555", lw=0.5, ls="--", zorder=4))
        lab = ABBR.get(o["class"]) or re.sub(r"([a-z])([A-Z])", r"\1 \2", o["class"].replace("Factory", "")).split(" ")[-1].lower()
        if CLEAN and f.area > 0.03:
            c = f.centroid; ax.text(c.x, c.y, lab, fontsize=6 if is_sink else 5, ha="center", va="center", zorder=6, color="#123" if is_sink else "#333", style="normal" if is_sink else "italic", weight="bold" if is_sink else "normal")
    flags = []
    for rk, r in res.items():
        if not r["flags"] or CLEAN: continue
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

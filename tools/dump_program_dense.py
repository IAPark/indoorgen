"""Dense-furnishing variant of the constraint program: data/home_program_dense.json = home.py's program + supplementary rules
(more storage/seating/plants/lamps per room type; same node/bounds format as tools/dump_program.py output).
usage (cwd = python Infinigen checkout):  python tools/dump_program_dense.py OUT.json
The original data/home_program.json stays untouched; INFINIGEN_PROGRAM=1 selects it at run time."""
import sys, json, itertools
sys.path.insert(0, __import__('os').path.expanduser('~/work/ref/scripts'))
import dump_program as dp
from dump_program import *   # Ser, NamedFloat, PARAM_KEYS, DEFAULTS, hc, gi, init, r, cl
from infinigen.assets.objects import appliances, bathroom, lamp, seating, shelves, tables, tableware
from infinigen.core.tags import Semantics
from infinigen_examples.constraints import util as cu

def extras(cons):
    rooms = cl.scene()[{Semantics.Room, -Semantics.Object}]
    obj = cl.scene()[{Semantics.Object, -Semantics.Room}]
    furniture = obj[Semantics.Furniture].related_to(rooms, cu.on_floor)
    wallfurn = furniture.related_to(rooms, cu.against_wall)
    storage = furniture[Semantics.Storage]
    storage_freestanding = storage.related_to(rooms, cu.against_wall)
    cutters = cl.scene()[Semantics.Cutter]
    doors = cutters[Semantics.Door]
    window = cutters[Semantics.Window]
    C, S = cons.constraints, cons.score_terms

    sofas = furniture[seating.SofaFactory]
    beds = wallfurn[Semantics.Bed]
    tvstands = wallfurn[shelves.TVStandFactory]
    coffeetables = furniture[tables.CoffeeTableFactory]
    desks = wallfurn[shelves.SimpleDeskFactory]
    sidetables = furniture[tables.SideTableFactory].related_to(wallfurn, cu.leftright_leftright)
    rugs = obj[__import__('infinigen.assets.objects.elements', fromlist=['x']).RugFactory].related_to(rooms, cu.on_floor)
    floor_lamps = (obj[Semantics.Lighting][lamp.FloorLampFactory].related_to(rooms, cu.on_floor).related_to(rooms, cu.against_wall))
    big_plants = (obj[tableware.LargePlantContainerFactory].related_to(rooms, cu.on_floor).related_to(rooms, cu.against_wall))
    armchairs = furniture[seating.ArmChairFactory]
    livingrooms = rooms[Semantics.LivingRoom].excludes(cu.room_types)
    bedrooms = rooms[Semantics.Bedroom].excludes(cu.room_types)
    diningrooms = rooms[Semantics.DiningRoom].excludes(cu.room_types)
    kitchens = rooms[Semantics.Kitchen].excludes(cu.room_types)
    bathrooms = rooms[Semantics.Bathroom].excludes(cu.room_types)
    shelfitem = lambda s: obj[Semantics.OfficeShelfItem].related_to(s, cu.on).count() >= 0

    tvs_n = lambda r: tvstands.related_to(r).count().equals(1)
    # ---- lamps: never in bathrooms (original: 0-2 desk lamps on any storage / desk in any room)
    nobath = rooms[-Semantics.Bathroom]
    lamps = lights_ = obj[Semantics.Lighting][lamp.DeskLampFactory].related_to(furniture, cu.ontop)
    C["lamps"] = nobath.all(lambda r: (
        lamps.related_to(storage.related_to(r)).count().in_range(0, 2)
        * lamps.related_to(desks.related_to(r, cu.on), cu.ontop).count().in_range(0, 1)
        * lamps.related_to(storage.related_to(r)).all(lambda l: l.distance(r, cu.floortags).in_range(0.5, 1.5))))
    C["no_bath_lamps"] = bathrooms.all(lambda r: lamps.related_to(furniture.related_to(r)).count().in_range(0, 0))

    # ---- plants: up to 2 big floor plants per room (original 0-1), small plants as before
    small_plants = obj[tableware.PlantContainerFactory].related_to(storage, cu.ontop)
    C["plants"] = rooms.all(lambda r: (big_plants.related_to(r).count().in_range(0, 2)
                                       * small_plants.related_to(storage.related_to(r)).count().in_range(0, 5)))

    # ---- living room (replaces "livingroom"): more storage, coffee table, side tables, 0-2 armchairs facing the coffee table, floor lamps near seating
    C["livingroom"] = livingrooms.all(lambda r: (
        storage_freestanding.related_to(r).count().in_range(2, 7)
        * tvs_n(r)
        * sidetables.related_to(sofas.related_to(r)).count().in_range(1, 2)
        * desks.related_to(r).count().in_range(0, 1)
        * coffeetables.related_to(r).count().in_range(1, 1)
        * coffeetables.related_to(r).all(lambda t: obj[Semantics.OfficeShelfItem].related_to(t, cu.on).count().in_range(0, 3))
        * rugs.related_to(r).count().in_range(0, 2)
        * armchairs.related_to(r).count().in_range(0, 2)
        * floor_lamps.related_to(r).count().in_range(0, 2)
        * armchairs.related_to(r).all(lambda a: ((cl.accessibility_cost(a, r, dist=1).in_range(0, 0.5))
                                                 * (cl.focus_score(a, coffeetables.related_to(r)) < 0.5)))))
    S["livingroom_dense"] = livingrooms.mean(lambda r: (
        armchairs.related_to(r).mean(lambda a: (
            a.distance(coffeetables.related_to(r)).hinge(0.4, 1.8).minimize(weight=5)
            + cl.focus_score(a, coffeetables.related_to(r)).minimize(weight=5)))
        + floor_lamps.related_to(r).mean(lambda l: l.distance(sofas.related_to(r)).hinge(0.0, 1.5).minimize(weight=3))))

    # ---- bedroom (replaces "bedroom"): 1-2 nightstands, dresser/wardrobe storage, armchair, floor lamp
    C["bedroom"] = bedrooms.all(lambda r: (
        beds.related_to(r).count().in_range(1, 2)
        * sidetables.related_to(beds.related_to(r)).count().in_range(1, 2)
        * rugs.related_to(r).count().in_range(0, 1)
        * desks.related_to(r).count().in_range(0, 1)
        * storage_freestanding.related_to(r).count().in_range(2, 6)
        * floor_lamps.related_to(r).count().in_range(0, 1)
        * storage.related_to(r).all(lambda s: shelfitem(s))))
    S["bedroom_dense"] = bedrooms.mean(lambda r: sidetables.related_to(beds.related_to(r)).count().maximize(weight=3))

    # ---- kitchen (replaces "kitchen_counters"): 2-3 wall counter runs; cabinets
    wallcounter = furniture[Semantics.KitchenCounter][shelves.KitchenSpaceFactory].related_to(rooms, cu.against_wall)
    island = furniture[Semantics.KitchenCounter][shelves.KitchenIslandFactory]
    C["kitchen_counters"] = kitchens.all(lambda r: (wallcounter.related_to(r).count().in_range(1, 3)
                                                    * island.related_to(r).count().in_range(0, 1)))
    C["kitchen_dense"] = kitchens.all(lambda r: wallfurn[shelves.KitchenCabinetFactory].related_to(r).count().in_range(0, 2))

    # ---- dining room: sideboard/cabinets on free walls
    C["diningroom_dense"] = diningrooms.all(lambda r: storage_freestanding.related_to(r).count().in_range(2, 4))
    # ---- bathroom: storage
    C["bathroom_dense"] = bathrooms.all(lambda r: storage_freestanding.related_to(r).count().in_range(1, 2))

def build_dense(bits):
    flags = dict(zip(PARAM_KEYS[5:], bits))
    def fake_params():
        p = {k: NamedFloat(DEFAULTS[k], k) for k in DEFAULTS}
        p.update(flags); return p
    hc.sample_home_constraint_params = fake_params
    cnt = {"i": 0}
    names = ["sofa_back_margin", "sofa_side_margin"]
    def fake_uniform(a=0.0, b=1.0):
        v = NamedFloat(0.2, names[cnt["i"]]); cnt["i"] += 1; return v
    hc.uniform = fake_uniform
    cons = hc.home_furniture_constraints()
    extras(cons)
    s = Ser()
    out = {"constraints": {k: s.val(v) for k, v in cons.constraints.items()},
           "score_terms": {k: s.val(v) for k, v in cons.score_terms.items()}}
    out["nodes"] = s.nodes
    bounds = r.constraint_bounds(cons)
    out["bounds"] = [{"domain": s.domain(b.domain), "low": b.low, "high": b.high} for b in bounds]
    return out

if __name__ == "__main__":
    init.apply_gin_configs(configs=["base_indoors.gin","fast_solve.gin","singleroom.gin"], overrides=["compose_indoors.terrain_enabled=False"],
        config_folders=["infinigen_examples/configs_indoor","infinigen_examples/configs_nature"])
    base = json.load(open(sys.argv[2])) if len(sys.argv) > 2 else None   # reuse stages/used_as from the original export
    result = {"variants": {}}
    for bits in itertools.product([False, True], repeat=5):
        key = "".join("1" if b else "0" for b in bits)
        result["variants"][key] = build_dense(bits)
        print("variant", key, len(result["variants"][key]["nodes"]), "nodes", len(result["variants"][key]["bounds"]), "bounds", flush=True)
    s = Ser()
    result["stages"] = {k: s.domain(v) for k, v in gi.default_greedy_stages().items()}
    from infinigen_examples.constraints.semantics import home_asset_usage
    result["used_as"] = {k.name: sorted(c.__name__ for c in v) for k, v in home_asset_usage().items()}
    # cooking pots belong in kitchens (KitchenCounterItem keeps them via Cookware), not on dining / coffee tables
    result["used_as"]["TableDisplayItem"] = [c for c in result["used_as"]["TableDisplayItem"] if c != "PotFactory"]
    result["used_as"]["Dishware"] = [c for c in result["used_as"]["Dishware"] if c not in ("PotFactory", "PanFactory")]
    json.dump(result, open(sys.argv[1], "w"))

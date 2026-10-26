"""usage: python dumpmats_blend.py scene.blend out.json  -- dump room material node trees of a python scene.blend"""
import sys, json, bpy
sys.path.insert(0, __import__("os").path.dirname(__import__("os").path.abspath(__file__)))
bpy.ops.wm.open_mainfile(filepath=sys.argv[1])
import roommats
json.dump(roommats.dump_room_materials(), open(sys.argv[2], "w"))

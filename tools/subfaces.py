"""subfaces.py in.json out.json f1,f2,...: extract faces by index (vertices compacted)"""
import sys, json
d = json.load(open(sys.argv[1])); faces = [int(x) for x in sys.argv[3].split(",")]
used = sorted({i for f in faces for i in d['p'][f]}); mp = {o: n for n, o in enumerate(used)}
json.dump({"v": [d['v'][i] for i in used], "p": [[mp[i] for i in d['p'][f]] for f in faces]}, open(sys.argv[2], "w"))

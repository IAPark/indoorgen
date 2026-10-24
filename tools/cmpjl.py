"""cmpjl.py file.jsonl seed rs.json : write py entry to /tmp/f_pyx.json then compare"""
import sys, json
for l in open(sys.argv[1]):
    j = json.loads(l)
    if j["fseed"] == int(sys.argv[2]): json.dump(j, open("/tmp/f_pyx.json", "w"))

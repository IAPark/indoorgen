"""blender_server.py [QUEUE_DIR]: a long-running Blender (bpy) process that renders jobs with render_rust.py, so Blender's startup and the
Infinigen imports (~15-25 s of every render_rust.py run) are paid once. Jobs are files in QUEUE_DIR/jobs/<id>.json:
    {"argv": [render_rust.py arguments...], "env": {"NAME": "value", ...}}
For each job the scene is reset (factory settings, empty), the job's environment is applied (and restored after), render_rust.py runs in
this process, its output goes to QUEUE_DIR/done/<id>.log and the result to QUEUE_DIR/done/<id>.json ({"rc", "seconds"}).
Client: tools/render/brender.sh. Stop: touch QUEUE_DIR/stop."""
import contextlib
import io
import json
import os
import runpy
import sys
import time
import traceback

QUEUE = sys.argv[-1] if len(sys.argv) > 1 and not sys.argv[-1].endswith(".py") else os.path.expanduser("~/work/bench/bsrv")
HERE = os.path.dirname(os.path.abspath(__file__))
os.makedirs(os.path.join(QUEUE, "jobs"), exist_ok=True)
os.makedirs(os.path.join(QUEUE, "done"), exist_ok=True)
t0 = time.time()
import bpy  # noqa: E402

sys.path.insert(0, HERE)
# warm the imports render_rust.py makes (infinigen materials, lighting, the Blender 5 shims)
_warm = io.StringIO()
with contextlib.redirect_stdout(_warm), contextlib.redirect_stderr(_warm):
    try:
        import bl5compat  # noqa: F401
        import infinigen.assets.materials  # noqa: F401
        from infinigen.assets.lighting import sky_lighting  # noqa: F401
    except Exception as e:
        print("warm import", repr(e))
print("BSRV ready in %.1fs, queue %s" % (time.time() - t0, QUEUE), flush=True)
with open(os.path.join(QUEUE, "ready"), "w") as f:
    f.write(str(os.getpid()))

# scene reuse: render_rust.py is split at its "# ==== VIEW ====" marker. The first job on a scene runs the setup part, saves the
# prepared scene to QUEUE/scenes/<key>.blend and keeps the setup's globals; later jobs with the same key (same glb + mtime, same
# environment and non-view arguments) reopen that .blend and run only the view part (camera, outdoor, exposure, render).
import hashlib
SRC = os.path.join(HERE, "render_rust.py")
MARK = "# ==== VIEW ===="
SCENES = os.path.join(QUEUE, "scenes"); os.makedirs(SCENES, exist_ok=True)
_cache = {}  # key -> (blend path, setup globals)
VIEW_FLAGS = {"--view": 1, "--samples": 1, "--cam": 1, "--exposure": 1}


def setup_key(job):
    a = list(job["argv"])
    keep, i = [], 0
    pos = [x for x in a if not x.startswith("--")]
    while i < len(a):
        if a[i] in VIEW_FLAGS: i += 2; continue
        keep.append(a[i]); i += 1
    glb = next((x for x in a if x.endswith(".glb")), "")
    outp = next((x for x in a if x.endswith(".png")), None)
    keep = [x for x in keep if x != outp]
    env = {k: v for k, v in job.get("env", {}).items() if k not in ("PHASES", "RES_PCT", "VIEWYAW_FORCE")}
    raw = json.dumps([glb, os.path.getmtime(glb) if os.path.exists(glb) else 0, keep, sorted(env.items())])
    return hashlib.sha1(raw.encode()).hexdigest()[:16]


def run_job(job):
    code = open(SRC).read()
    head, sep, view = code.partition(MARK)
    view = "\n" * (head.count("\n") + 1) + view.split("\n", 1)[1] if sep else view  # (rest of the marker line dropped; padded so tracebacks keep render_rust.py's line numbers)
    key = setup_key(job)
    hit = _cache.get(key)
    if hit and os.path.exists(hit[0]) and not os.environ.get("BSRV_NOREUSE"):
        t = time.time()
        bpy.ops.wm.open_mainfile(filepath=hit[0])
        g = hit[1]  # (the same dict the setup's functions see as their globals)
        g["sc"] = bpy.context.scene
        g["args"] = g["ap"].parse_args(sys.argv[1:])
        g["_T0"] = [time.time()]
        print("BSRV reuse scene %s (open %.1fs)" % (key, time.time() - t), flush=True)
        exec(compile(view, SRC, "exec"), g)
        return
    bpy.ops.wm.read_factory_settings(use_empty=True)
    g = {"__name__": "__main__", "__file__": SRC}
    exec(compile(head, SRC, "exec"), g)
    if sep:
        path = os.path.join(SCENES, key + ".blend")
        t = time.time()
        bpy.ops.wm.save_as_mainfile(filepath=path, copy=True, compress=False)
        _cache[key] = (path, g)
        print("BSRV saved scene %s (%.1fs)" % (key, time.time() - t), flush=True)
        exec(compile(view, SRC, "exec"), g)


while not os.path.exists(os.path.join(QUEUE, "stop")):
    jobs = sorted(p for p in os.listdir(os.path.join(QUEUE, "jobs")) if p.endswith(".json"))
    if not jobs:
        time.sleep(0.1)
        continue
    name = jobs[0]
    jid = name[:-5]
    path = os.path.join(QUEUE, "jobs", name)
    try:
        job = json.load(open(path))
    except Exception:
        time.sleep(0.1)  # still being written
        continue
    os.remove(path)
    saved_env = {k: os.environ.get(k) for k in job.get("env", {})}
    os.environ.update({k: str(v) for k, v in job.get("env", {}).items()})
    log = open(os.path.join(QUEUE, "done", jid + ".log"), "w")
    ts = time.time()
    rc = 0
    with contextlib.redirect_stdout(log), contextlib.redirect_stderr(log):
        try:
            sys.argv = [os.path.join(HERE, "render_rust.py")] + list(job["argv"])
            run_job(job)
        except SystemExit as e:
            rc = e.code if isinstance(e.code, int) else (0 if e.code is None else 1)
        except Exception:
            traceback.print_exc()
            rc = 1
    log.close()
    for k, v in saved_env.items():
        if v is None:
            os.environ.pop(k, None)
        else:
            os.environ[k] = v
    res = {"rc": rc, "seconds": round(time.time() - ts, 2)}
    with open(os.path.join(QUEUE, "done", jid + ".json.tmp"), "w") as f:
        json.dump(res, f)
    os.replace(os.path.join(QUEUE, "done", jid + ".json.tmp"), os.path.join(QUEUE, "done", jid + ".json"))
    print("BSRV job", jid, res, flush=True)
print("BSRV stop", flush=True)

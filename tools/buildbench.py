"""buildbench.py [files...]: time an incremental `cargo build --profile fast --bin gen2` after a trivial body edit in each file (min of 2)."""
import re, subprocess, sys, time, os
root = subprocess.run(['git', 'rev-parse', '--show-toplevel'], capture_output=True, text=True).stdout.strip()
files = sys.argv[1:] or ['crates/ig_seat/src/chair.rs', 'crates/ig_storage/src/toiletx.rs', 'crates/ig_gn/src/eval.rs', 'crates/ig_furn/src/solver.rs', 'crates/ig_core/src/mesh.rs', 'crates/ig_kit/src/bevelfull.rs']
env = dict(os.environ); env['PATH'] = os.path.expanduser('~/.cargo/bin') + ':' + env['PATH']
def build():
    t = time.time(); r = subprocess.run(['cargo', 'build', '--profile', 'fast', '--bin', 'gen2'], cwd=root, env=env, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2000:]
    return time.time() - t
build()
for f in files:
    p = os.path.join(root, f); s = open(p).read()
    ms = list(re.finditer(r'\n(pub )?fn [^\n]*\{\n', s)); m = ms[min(2, len(ms) - 1)]
    ts = []
    for k in range(2):
        open(p, 'w').write(s[:m.end()] + f'    std::hint::black_box({k + 7}u8);\n' + s[m.end():]); ts.append(build())
    open(p, 'w').write(s); build()
    print(f"{f:45s} {min(ts):6.2f}s")

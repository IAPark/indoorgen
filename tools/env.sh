# Sourced by tools/*.sh: resolves repository-relative locations and the optional external tools.
# Usage from a script (bash or zsh, executed not sourced):  IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# Per-machine overrides go in $IG_ROOT/.indoorgen.env (gitignored) or the environment:
#   INDOORGEN_ASSETS    Rocketbox avatars (default $IG_ROOT/assets/rocketbox; fetched by tools/people/fetch_rocketbox.sh)
#   INDOORGEN_REF       checkout of the Python Infinigen (+ .venv / .venv52) used as reference/oracle and for Cycles materials (optional)
#   INDOORGEN_REFCACHE  cached Python reference meshes for refcmp (default $INDOORGEN_DATA/refcache)
#   INDOORGEN_ORACLES   Python oracle dumps for lockstep/phcheck/countbench (default $INDOORGEN_DATA/ref)
#   INDOORGEN_DATA      parent of the two above (default $IG_ROOT/local-data, gitignored)
#   PY_REF              python with bpy 4.2 + Infinigen (default $INDOORGEN_REF/.venv/bin/python)
#   BLENDER_PY          python with bpy 5.2 + Infinigen (--no-deps) used by tools/render/*.py (default $INDOORGEN_REF/.venv52/bin/python)
IG_ROOT=$(cd "$IG_TOOLS/.." && pwd); export IG_ROOT IG_TOOLS
[ -f "$IG_ROOT/.indoorgen.env" ] && . "$IG_ROOT/.indoorgen.env"
: "${INDOORGEN_DATA:=$IG_ROOT/local-data}"
: "${INDOORGEN_ASSETS:=$IG_ROOT/assets/rocketbox}"
: "${INDOORGEN_REF:=$HOME/infinigen-ref}"
: "${INDOORGEN_REFCACHE:=$INDOORGEN_DATA/refcache}"
: "${INDOORGEN_ORACLES:=$INDOORGEN_DATA/ref}"
: "${PY_REF:=$INDOORGEN_REF/.venv/bin/python}"
: "${BLENDER_PY:=$INDOORGEN_REF/.venv52/bin/python}"
export INDOORGEN_DATA INDOORGEN_ASSETS INDOORGEN_REF INDOORGEN_REFCACHE INDOORGEN_ORACLES PY_REF BLENDER_PY
PATH=$HOME/.cargo/bin:$PATH

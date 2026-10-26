#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# lockstep.sh ROOM SEED [oracles]: like $INDOORGEN_ORACLES/cmpcase.sh but each python oracle is opt-in.
# oracles: comma list of state0,ph,ctor (loaded from the python dumps); ctoremu = Rust constructor-draw emulation (no CTORWORDS file).
R=$1; S=$2; O=${3:-state0,ph,ctor}
P=$INDOORGEN_ORACLES/traces/case_${R}_${S}
cd "$(git rev-parse --show-toplevel)"
unset STATE0 PHDUMP CTORWORDS CTOREMU
[[ $O == *state0* ]] && export STATE0=$([ -f $INDOORGEN_ORACLES/traces/s0e_${R}_${S}.json ] && echo $INDOORGEN_ORACLES/traces/s0e_${R}_${S}.json || echo ${P}_state0.json)
[[ $O == *ph* ]] && export PHDUMP=${P}_ph.jsonl
[[ $O == *ctor* ]] && export CTORWORDS=${P}_ctor.json
[[ $O == *emu* ]] && export CTOREMU=1
W=$(mktemp -d /tmp/lockstep.XXXXXX)
RNGTR=200 ${GEN2:-./target/fast/gen2} $S $R $W/x.glb > /dev/null 2> $W/trace.txt
python3 $IG_TOOLS/oracle/cmp_fast.py $P.trace $W/trace.txt
[ -n "$KEEPTRACE" ] && cp $W/trace.txt /tmp/rs_trace_$R$S.txt
rm -rf $W

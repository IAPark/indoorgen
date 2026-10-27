#!/bin/zsh
# build the browser module: web/pkg/ig_wasm.{js,wasm,d.ts}
#   tools/wasm/build.sh            release build + wasm-bindgen (--target web) + wasm-opt
#   WASM_TARGET=nodejs tools/wasm/build.sh   node build into web/pkg-node (for tools/wasm/node_run.mjs)
#   WASM_OPT=-Oz ...               wasm-opt level (default -O3; set to "none" to skip)
# needs: rustup target add wasm32-unknown-unknown; rustup component add llvm-tools; cargo install wasm-bindgen-cli (same version as the
# wasm-bindgen crate in Cargo.lock); brew install binaryen (wasm-opt)
set -e
IG_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$IG_ROOT"
. tools/wasm/env.sh
export TMPDIR=$IG_ROOT/tmp; mkdir -p $TMPDIR
PROFILE=${WASM_PROFILE:-wasm}
cargo build --profile $PROFILE --target wasm32-unknown-unknown -p ig_wasm --no-default-features
TARGET=${WASM_TARGET:-web}
OUT=web/pkg; [ "$TARGET" = nodejs ] && OUT=web/pkg-node
rm -rf $OUT; mkdir -p $OUT
RAW=$CARGO_TARGET_DIR/wasm32-unknown-unknown/$PROFILE/ig_wasm.wasm
wasm-bindgen ${WASM_KEEP_DEBUG:+--keep-debug} --target $TARGET --out-dir $OUT --out-name ig_wasm $RAW
OPT=${WASM_OPT:--O3}
if [ "$OPT" != none ]; then
  wasm-opt $OPT --enable-simd --enable-bulk-memory --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int --enable-multivalue --enable-reference-types ${WASM_KEEP_DEBUG:---strip-debug} -o $OUT/ig_wasm_bg.opt.wasm $OUT/ig_wasm_bg.wasm && mv $OUT/ig_wasm_bg.opt.wasm $OUT/ig_wasm_bg.wasm
fi
ls -l $OUT

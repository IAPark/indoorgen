#!/bin/zsh
# the baked renderer for a page: web/pkg-rt/preview.{js,wasm}
set -e
D=${0:A:h}/../preview; R=${0:A:h}/../..
cd $D; ~/.cargo/bin/cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir $R/web/pkg-rt --out-name preview ${CARGO_TARGET_DIR:-$D/target}/wasm32-unknown-unknown/release/preview.wasm
wasm-opt -O3 --enable-simd --enable-bulk-memory --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int --enable-reference-types --enable-multivalue $R/web/pkg-rt/preview_bg.wasm -o $R/web/pkg-rt/preview_bg.wasm
ls -l $R/web/pkg-rt

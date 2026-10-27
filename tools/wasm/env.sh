# environment for building the wasm module (source me): . tools/wasm/env.sh
. "$HOME/.cargo/env"
IG_WASM_ROOT=$(cd "$(dirname "${(%):-%x}")/../.." && pwd)
export CARGO_TARGET_DIR=$IG_WASM_ROOT/target-wasm
SYSROOT_BIN=$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/host: //p')/bin
# meshopt (C++ via cc): llvm-ar from the rustup llvm-tools component ("rustup component add llvm-tools"); the stripped wasm32 headers of
# the crate lack floorf / log2f, which are compiler builtins / provided by compiler_builtins
export AR_wasm32_unknown_unknown=$SYSROOT_BIN/llvm-ar
export CXXFLAGS_wasm32_unknown_unknown="-Dfloorf=__builtin_floorf -Dceilf=__builtin_ceilf -Dlog2f=__builtin_log2f -Dfrexpf=__builtin_frexpf -Dldexpf=__builtin_ldexpf -Dexp2f=__builtin_exp2f -Dpowf=__builtin_powf -Dcosf=__builtin_cosf -Dsinf=__builtin_sinf -Dtanf=__builtin_tanf -Dacosf=__builtin_acosf -Dasinf=__builtin_asinf -Datanf=__builtin_atanf -Datan2f=__builtin_atan2f -Dexpf=__builtin_expf -Dlogf=__builtin_logf -Dlog10f=__builtin_log10f -Dfmodf=__builtin_fmodf -Droundf=__builtin_roundf -Dtruncf=__builtin_truncf -Dsqrtf=__builtin_sqrtf -Dfabsf=__builtin_fabsf -Dhypotf=__builtin_hypotf"
export CFLAGS_wasm32_unknown_unknown="-Dfloorf=__builtin_floorf -Dceilf=__builtin_ceilf -Dlog2f=__builtin_log2f -Dfrexpf=__builtin_frexpf -Dldexpf=__builtin_ldexpf -Dexp2f=__builtin_exp2f -Dpowf=__builtin_powf -Dcosf=__builtin_cosf -Dsinf=__builtin_sinf -Dtanf=__builtin_tanf -Dacosf=__builtin_acosf -Dasinf=__builtin_asinf -Datanf=__builtin_atanf -Datan2f=__builtin_atan2f -Dexpf=__builtin_expf -Dlogf=__builtin_logf -Dlog10f=__builtin_log10f -Dfmodf=__builtin_fmodf -Droundf=__builtin_roundf -Dtruncf=__builtin_truncf -Dsqrtf=__builtin_sqrtf -Dfabsf=__builtin_fabsf -Dhypotf=__builtin_hypotf"
# 64 MiB stack (deep recursion in the booleans / bevel code), memory may grow to the 4 GiB wasm32 limit
export RUSTFLAGS="-C link-arg=-zstack-size=67108864 -C link-arg=--max-memory=4294967296 -C target-feature=+simd128"

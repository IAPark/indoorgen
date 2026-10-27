#!/bin/zsh
# raw-deflate the json data files embedded in the wasm module (data/wasm/*.json.deflate, decoded with miniz_oxide at startup)
cd "$(dirname "$0")/../.." || exit 1
python3 - <<'PY'
import zlib
for n in ['home_program_dense','home_program','asset_samples','asset_bbox_table']:
    d=open(f'data/{n}.json','rb').read()
    c=zlib.compressobj(9,zlib.DEFLATED,-15); z=c.compress(d)+c.flush()
    open(f'data/wasm/{n}.json.deflate','wb').write(z)
PY

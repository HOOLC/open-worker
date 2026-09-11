#!/usr/bin/env python3
"""Architecture guard: the app and web examples must share their visual implementation."""
from pathlib import Path
import re
import tomllib
ROOT=Path(__file__).resolve().parents[2]
ui=tomllib.loads((ROOT/'crates/zork-ui/Cargo.toml').read_text())
assert not {'reqwest','rusqlite','zork-config','zork-mesh','zork-station','axum'} & ui['dependencies'].keys()
native=tomllib.loads((ROOT/'crates/zork-gui/Cargo.toml').read_text())
web=tomllib.loads((ROOT/'crates/zork-gui-web/Cargo.toml').read_text())
assert native['dependencies']['zork-ui']['path']=='../zork-ui'
assert web['target']['cfg(target_family = "wasm")']['dependencies']['zork-ui']['path']=='../zork-ui'
for name in ['activity','brand','message','selection','selector_menu','text_input']:
 p=ROOT/f'crates/zork-gui/src/components/{name}.rs';source=p.read_text()
 # The native message adapter may also re-export its host-side cache types.
 # This permits declarations only, never a copied visual implementation.
 if name=='message':
  source=re.sub(r'pub use super::transcript_cache::\{[\w\s,]+\};\s*','',source)
 s=source.strip().splitlines()
 assert s[0]==f'pub use zork_ui::components::{name}::*;',p
 assert len(s)<3,(p,'app copied the component implementation')
assert (ROOT/'crates/zork-gui/assets').resolve()==(ROOT/'crates/zork-ui/assets').resolve()
assert 'pub use zork_ui::components;' in (ROOT/'crates/zork-gui-web/src/lib.rs').read_text()
print('PASS shared component package: one visual source, no Zork service dependencies')

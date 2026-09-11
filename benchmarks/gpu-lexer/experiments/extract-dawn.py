"""Normalize only entry-point names in the captured upstream-generated MSL."""
from pathlib import Path
import json,re,hashlib
root=Path(__file__).resolve().parents[1]
records=json.loads((root/'experiments/dawn-shaders.json').read_text())
checksums={}
for row in records:
    source=row['text']
    if '#include <metal_stdlib>' not in source:continue
    stage=re.search(r'void ([a-g])_inner\(',source)[1]
    source=re.sub(r'kernel void dawn_entry_point_[a-zA-Z0-9_]+\(',f'kernel void gpu_lexer_{stage}(',source)
    path=root/f'assets/dawn/{stage}.metal';path.write_text(source)
    checksums[stage]=hashlib.sha256(source.encode()).hexdigest()
assert len(checksums)==7
(root/'assets/dawn/checksums.json').write_text(json.dumps(checksums,indent=2)+'\n')

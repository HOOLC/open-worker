"""Derive a layout-only shader variant from the unchanged pinned WGSL."""
from pathlib import Path
import re
root=Path(__file__).resolve().parent
s=(root/'assets/model.wgsl').read_text()
s=s.replace('var<workgroup> j:array<vec4<f32>,1024>;', ''.join(f'var<workgroup> j{c}:array<f32,1024>;' for c in 'xyzw')+'''
fn load_j(idx:u32)->vec4<f32>{return vec4<f32>(jx[idx],jy[idx],jz[idx],jw[idx]);}
fn store_j(idx:u32,v:vec4<f32>){jx[idx]=v.x;jy[idx]=v.y;jz[idx]=v.z;jw[idx]=v.w;}
''')
s=re.sub(r'j\[([^\]]+)\]\.yz',r'vec2<f32>(jy[\1],jz[\1])',s)
s=re.sub(r'j\[([^\]]+)\]\.([xyzw])',r'j\2[\1]',s)
s=re.sub(r'j\[([^\]]+)\]=([^;]+);',r'store_j(\1,\2);',s)
s=re.sub(r'j\[([^\]]+)\]',r'load_j(\1)',s)
assert 'j[' not in s
(root/'assets/soa.wgsl').write_text(s)
# The upstream select evaluates an out-of-range tail-token load even when Sb is
# false. A branch avoids the load, so the fixed, host-validated shader can also be
# compiled without injected array clamps. All other indexes are range-guarded or
# derive from fixed tensor dimensions; see experiments/BOUNDS-AUDIT.md.
original=(root/'assets/model.wgsl').read_text()
tail='let wd=select(1u,M[qa*2u]&3u,Sb);'
assert original.count(tail)==1
safe=original.replace(tail,'var wd=1u;if(Sb){wd=M[qa*2u]&3u;}')
(root/'assets/validated.wgsl').write_text(safe)
(root/'assets/tiny.wgsl').write_text(safe+'\n'+(root/'src/tiny.wgsl').read_text())

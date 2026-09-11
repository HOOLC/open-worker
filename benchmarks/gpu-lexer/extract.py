"""Extract unchanged WGSL and decoded f32 weights from pinned npm distribution."""
import hashlib, json, re, struct, tarfile
from pathlib import Path
root = Path(__file__).resolve().parent
checksums = json.loads((root / 'assets/checksums.json').read_text())
archive = root / 'vendor/gpu-lexer-0.0.2.tgz'
if hashlib.sha256(archive.read_bytes()).hexdigest() != checksums['vendor/gpu-lexer-0.0.2.tgz']:
    raise SystemExit('Pinned gpu-lexer archive checksum mismatch')
# dist is generated and ignored. Restore only the two known distribution files.
with tarfile.open(archive, 'r:gz') as package:
    for name in ['package/dist/index.js', 'package/dist/index.d.ts']:
        member = package.getmember(name)
        if not member.isfile():
            raise SystemExit('Expected a regular distribution file: ' + name)
        source = package.extractfile(member)
        if source is None:
            raise SystemExit('Missing distribution file: ' + name)
        contents = source.read()
        destination = root / 'vendor' / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(contents)
source_path = root / 'vendor/package/dist/index.js'
if hashlib.sha256(source_path.read_bytes()).hexdigest() != checksums['vendor/package/dist/index.js']:
    raise SystemExit('Pinned gpu-lexer JavaScript checksum mismatch')
src = source_path.read_text()
scales = [float(v) for v in re.search(r'var x=\[(.*?)\],EA=', src)[1].split(',')]
encoded = json.loads(re.search(r'EA=(".*?");var SA=', src)[1])
shader = json.loads(re.search(r'var SA=(".*?"),fA=', src)[1])
weights = []
start = 0
for end, scale in zip(scales[::2], scales[1::2]):
    for ch in encoded[start:int(end)]:
        c = ord(ch)
        value = c - 71 if c >= 97 else c - 65 if c >= 65 else c + 4 if c >= 48 else 62 if c == 45 else 63
        weights.append((-(value + 1) / 2 if value & 1 else value / 2) * scale)
    start = int(end)
assert len(weights) == 41321
(root / 'assets/model.wgsl').write_text(shader)
(root / 'assets/weights.f32').write_bytes(struct.pack('<' + 'f' * len(weights), *weights))
files = ['vendor/gpu-lexer-0.0.2.tgz', 'vendor/package/dist/index.js', 'assets/model.wgsl', 'assets/weights.f32']
(root / 'assets/checksums.json').write_text(json.dumps({p: hashlib.sha256((root/p).read_bytes()).hexdigest() for p in files}, indent=2)+'\n')

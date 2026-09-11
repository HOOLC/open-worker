"""Generate exact byte doublings from the saved 16 KiB source snapshots."""
from pathlib import Path
import hashlib
import json
root = Path(__file__).resolve().parent
(root / 'fixtures/sweep').mkdir(exist_ok=True)
original = json.loads((root / 'fixtures/manifest.json').read_text())
rows = []
for size in [2**i for i in range(3, 14)]:
    for name in ['rust', 'typescript', 'json', 'yaml', 'python']:
        source = next(row for row in original if row['file'] == f'{name}-16384.txt')
        raw = (root / 'fixtures' / source['file']).read_bytes()[:size]
        text = raw.decode('utf-8', errors='ignore')
        padding = size - len(text.encode())
        text += ' ' * padding
        file = f'sweep/{name}-{size}.txt'
        (root / 'fixtures' / file).write_bytes(text.encode())
        rows.append(dict(file=file, language=source['language'], bytes_target=size,
                         source_fixture=source['file'], utf8_padding_bytes=padding,
                         sha256=hashlib.sha256(text.encode()).hexdigest()))
(root / 'fixtures/sweep-manifest.json').write_text(json.dumps(rows, indent=2) + '\n')

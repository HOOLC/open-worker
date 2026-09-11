"""Legacy preview snapshot, contained inside this workspace package."""
from pathlib import Path
import shutil
ROOT=Path(__file__).resolve().parents[1]
BUILD=ROOT/'dist'
TARGET=ROOT/'dist-preview'
if not (BUILD/'index.html').is_file():raise SystemExit('Build zork-design with pnpm build before syncing the preview.')
html=(BUILD/'index.html').read_text()
if 'type="module"' not in html:raise SystemExit('Refusing to publish a non-Vite handbook entry.')
TARGET.mkdir(parents=True,exist_ok=True)
shutil.copytree(BUILD,TARGET/'design',dirs_exist_ok=True)
# Both long-standing entry URLs open the same React application.
(TARGET/'index.html').write_text(html)
print('Updated the local React design preview from dist/.')

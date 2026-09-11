# /// script
# dependencies = ["fonttools==4.62.1"]
# ///
"""Materialize shared Inter faces and Web CJK weights from the source fonts.

Static Inter also avoids CoreText's expensive variable-font setup on each new
multiline message. The glyph outlines use the same default optical size (14).
"""
from pathlib import Path
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = Path(__file__).resolve().parents[2]
SHARED = ROOT / 'crates/zork-ui/assets/fonts'
WEB = ROOT / 'crates/zork-gui-web/assets'

for family, source, output, axes, italic in [
    ('NotoSansSC', WEB / 'NotoSansSC.ttf', WEB / 'static', {}, False),
    ('Inter', SHARED / 'InterVariable.ttf', SHARED / 'static', {'opsz': 14}, False),
    ('Inter', SHARED / 'InterVariable-Italic.ttf', SHARED / 'static', {'opsz': 14}, True),
]:
    output.mkdir(parents=True, exist_ok=True)
    for weight, style in [(400, 'Regular'), (500, 'Medium'), (600, 'SemiBold'), (700, 'Bold')]:
        suffix = '-Italic' if italic else ''
        dest = output / f'{family}-{weight}{suffix}.ttf'
        if dest.exists() and dest.stat().st_mtime >= source.stat().st_mtime:
            continue
        font = instantiateVariableFont(TTFont(source), dict(axes, wght=weight), inplace=True)
        name = 'Noto Sans SC' if family == 'NotoSansSC' else 'Inter Variable'
        face = ('Italic' if weight == 400 else style + ' Italic') if italic else style
        for record in font['name'].names:
            value = {1: name, 2: face, 16: name, 17: face,
                     6: family + '-' + face.replace(' ', '')}.get(record.nameID)
            if value is not None:
                record.string = value.encode(record.getEncoding())
        font.save(dest)
        print(dest.relative_to(ROOT), flush=True)

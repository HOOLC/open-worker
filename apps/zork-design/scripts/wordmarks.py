"""Construct editable Zork wordmark explorations from original letter paths."""
from pathlib import Path
import xml.etree.ElementTree as ET
import resvg_py
ROOT=Path(__file__).resolve().parents[1]
OPTIONS={
'fold':{'name':'折角 / Fold','viewBox':'0 0 136 44','letters':[
('z','M3 4H31V10L11 33H31V40H18L21 35L10 40H0V33L21 11H0V7Z'),
('o','M46 14H54L64 24V30Q64 40 54 40H46Q36 40 36 30V24Q36 14 46 14ZM47 21Q43 21 43 26V29Q43 33 47 33H53Q57 33 57 29V27L51 21Z'),
('r','M71 40V15H78V20Q81 15 87 15H94L89 22H86Q78 22 78 30V40Z'),
('k','M99 4H106V25L118 15H128L113 28L132 40H120L106 31V40H99Z')]},
'arc':{'name':'回转 / Turn','viewBox':'0 0 136 44','letters':[
('z','M4 4H31V11L11 32H31V40H0V33L21 12H0V8Q0 4 4 4Z'),
('o','M48 14H53Q64 14 64 25V29Q64 40 53 40H46Q35 40 35 29V25Q35 14 46 14ZM48 21Q42 21 42 27V28Q42 33 48 33H51Q57 33 57 28V27Q57 21 51 21Z'),
('r','M71 40V15H78V20Q82 15 88 15H94V22H87Q78 22 78 31V40Z'),
('k','M99 4H106V25L118 15H128L114 27L130 38L126 44L106 31V40H99Z')]}}
def svg(spec,color='#24272B'):
 paths=''.join(f'<path class="wordmark-letter letter-{letter}" fill-rule="evenodd" d="{d}"/>' for letter,d in spec['letters'])
 return f'<svg xmlns="http://www.w3.org/2000/svg" width="136" height="44" viewBox="{spec["viewBox"]}" role="img" aria-label="Zork"><title>Zork · {spec["name"]} · 字标探索 v2</title><g fill="{color}">{paths}</g></svg>'
for key,spec in OPTIONS.items():
 for suffix,color in [('', '#24272B'),('-reverse','#F6F3EA')]:
  (ROOT/f'wordmark/zork-{key}-v2{suffix}.svg').write_text(svg(spec,color)+'\n')
parts=['<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="730"><rect width="1200" height="730" fill="#F6F3EA"/>']
for row,(key,spec) in enumerate(OPTIONS.items()):
 y=35+row*345
 parts.append(f'<text x="42" y="{y+20}" fill="#62656B" font-family="Arial" font-size="18">{key.upper()} / V2</text>')
 for x,w in [(42,408),(570,170),(830,85),(1000,68)]:
  el=ET.fromstring(svg(spec));el.set('x',str(x));el.set('y',str(y+65));el.set('width',str(w));el.set('height',str(w*44/136));parts.append(ET.tostring(el,encoding='unicode'))
 parts.append(f'<text x="42" y="{y+258}" fill="#62656B" font-family="Arial" font-size="14">Large / UI / Compact / Small</text>')
parts.append('</svg>');sheet=''.join(parts)
(ROOT/'wordmark/comparison.svg').write_text(sheet)
(ROOT/'wordmark/comparison.png').write_bytes(resvg_py.svg_to_bytes(svg_string=sheet))
print('Built two custom letter-path directions with reverse variants and size proof.')

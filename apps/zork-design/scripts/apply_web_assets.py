"""Apply selected design assets to every current HTML prototype and preview source."""
from pathlib import Path
import hashlib,json,re,shutil,xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[1]
SITE=ROOT/'archive/reference-prototype'
ET.register_namespace('', 'http://www.w3.org/2000/svg')
changed=[]
def provenance(source):
 return source.relative_to(ROOT).as_posix() if isinstance(source,Path) else str(source)
def write(p,text,source):
 before=hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None
 p.parent.mkdir(parents=True,exist_ok=True);p.write_text(text)
 after=hashlib.sha256(p.read_bytes()).hexdigest()
 changed.append({'destination':str(p.relative_to(ROOT)),'source':provenance(source),'changed':before!=after,'sha256':after})
def copy(src,dst):
 before=hashlib.sha256(dst.read_bytes()).hexdigest() if dst.exists() else None
 dst.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(src,dst)
 digest=hashlib.sha256(dst.read_bytes()).hexdigest();changed.append({'destination':str(dst.relative_to(ROOT)),'source':str(src.relative_to(ROOT)),'changed':before!=digest,'sha256':digest})
word=(ROOT/'assets/brand/zork-wordmark-draft.svg').read_text()
mark=ET.parse(ROOT/'assets/brand/mark.svg').getroot();wm=ET.fromstring(word)
content=lambda el:''.join(ET.tostring(child,encoding='unicode') for child in el if not child.tag.endswith('title'))
lockup=f'<svg xmlns="http://www.w3.org/2000/svg" width="184" height="44" viewBox="0 0 184 44" role="img" aria-label="Zork"><title>Zork · 当前组合标志</title><g class="brand-mark" transform="scale(.34375)">{content(mark)}</g><g class="brand-wordmark" transform="translate(48 0)">{content(wm)}</g></svg>\n'
write(ROOT/'assets/brand/lockup.svg',lockup,'selected Fold v2 + unchanged brand mark')
for path in [ROOT/'prototype/gui.html',SITE/'gui.html']:
 text=path.read_text();replacement=word.strip().replace('<svg ','<svg class="brand-wordmark" ',1)
 text,n=re.subn(r'<svg class="brand-wordmark"[\s\S]*?</svg>',lambda _:replacement,text,count=1)
 assert n==1,path
 text=text.replace('<title>zork ·','<title>Zork ·')
 write(path,text,ROOT/'assets/brand/zork-wordmark-draft.svg')
for name in ['mark.svg','mark-micro.svg','mark-orange.svg','mark-reverse.svg']:
 copy(ROOT/'assets/brand'/name,SITE/'svg'/name)
copy(ROOT/'assets/brand/lockup.svg',SITE/'svg/lockup.svg')
copy(ROOT/'assets/brand/zork-wordmark-draft.svg',SITE/'svg/wordmark.svg')
copy(ROOT/'assets/brand/zork-wordmark-draft.svg',SITE/'gui/brand/zork-wordmark.svg')
for folder,destination in [('assets/icons/product','svg/icons'),('assets/icons/interface','gui/glyphs'),('assets/concepts/illustrations','svg/illustrations')]:
 for path in (ROOT/folder).glob('*.svg'):copy(path,SITE/destination/path.name)
copy(ROOT/'previews/scenes-v2.png',SITE/'previews/scenes.png')
copy(ROOT/'previews/functional-icons-v2.png',SITE/'previews/function-icons.png')
# The static overview contained old inline wordmark/scenes: replace it with the
# current generated visual material overview rather than keep contradictory art.
copy(ROOT/'previews/materials.svg',SITE/'svg/overview.svg')
copy(ROOT/'previews/materials.png',SITE/'previews/overview.png')
for path in [ROOT/'prototype/gui/navigation.css',SITE/'gui/navigation.css']:
 text=path.read_text().replace('620ms','700ms').replace('440ms','500ms').replace('animation-delay:90ms','animation-delay:140ms').replace('animation-delay:140ms}.brand:hover .letter-r','animation-delay:195ms}.brand:hover .letter-r').replace('animation-delay:180ms','animation-delay:250ms').replace('animation-delay:220ms','animation-delay:305ms')
 write(path,text,'motion/linked.svg timing')
mobile=ROOT/'mobile/prototype';copy(ROOT/'assets/brand/zork-wordmark-draft.svg',mobile/'assets/brand/zork-wordmark.svg');copy(ROOT/'assets/brand/lockup.svg',mobile/'assets/brand/lockup.svg')
p=mobile/'app.js';text=p.read_text().replace('<div class="brand"><img src="assets/mark.svg" alt="Zork 标识"><strong>Zork</strong></div>','<div class="brand"><img class="brand-mark" src="assets/mark.svg" alt=""><img class="brand-wordmark" src="assets/brand/zork-wordmark.svg" alt="Zork"></div>');write(p,text,'current Zork SVG wordmark')
p=mobile/'index.html';text=p.read_text().replace('<img src="assets/mark.svg" alt="">Zork <span>','<img class="preview-brand-lockup" src="assets/brand/lockup.svg" alt="Zork"><span>');write(p,text,'current Zork SVG lockup')
p=mobile/'style.css';text=p.read_text();marker='/* Applied current SVG branding */';text=text.split(marker)[0];text+='''\n/* Applied current SVG branding */
.brand .brand-mark{width:26px;height:26px}.brand .brand-wordmark{width:80px;height:26px;object-fit:contain}.preview-label .preview-brand-lockup{width:94px;height:24px}
@keyframes brand-icon-entry{0%{transform:rotate(-7deg)}65%{transform:rotate(2deg)}100%{transform:rotate(0)}}
@keyframes brand-word-entry{0%{transform:translateY(2px);opacity:.7}65%{transform:translateY(-1px);opacity:1}100%{transform:translateY(0);opacity:1}}
.brand-entry .brand-mark{animation:brand-icon-entry 680ms ease-out}.brand-entry .brand-wordmark{animation:brand-word-entry 675ms ease-out}
@media(prefers-reduced-motion:reduce){.brand .brand-mark,.brand-entry .brand-wordmark{animation:none}}
''';write(p,text,'motion icon / wordmark entry adaptations')
# Keep original source registry correct after replacing same-named files.
p=SITE/'manifest.json';data=json.loads(p.read_text())
for item in data['assets']:
 file=SITE/item['path']
 if file.exists():item['sha256']=hashlib.sha256(file.read_bytes()).hexdigest()
data['current_design_source']='design/README.md';write(p,json.dumps(data,ensure_ascii=False,indent=2)+'\n','current design assets')
p=ROOT/'assets/manifest.json';data=json.loads(p.read_text())
if not any(i['path']=='assets/brand/lockup.svg' for i in data['items']):data['items'].append({'path':'assets/brand/lockup.svg','category':'brand','status':'current-adopted-v2','source':'selected Fold v2 + unchanged mark','sha256':hashlib.sha256((ROOT/'assets/brand/lockup.svg').read_bytes()).hexdigest()})
for item in data['items']:
 if item['path']=='assets/brand/zork-wordmark-draft.svg':item['status']='current-adopted-v2'
write(p,json.dumps(data,ensure_ascii=False,indent=2)+'\n','selected current brand resources')
(ROOT/'asset-application.json').write_text(json.dumps({'version':'brand40','selected_wordmark':'fold-v2','brand_mark':'unchanged','web_surfaces':changed,'native':'in progress in implementation worktree; final validation pending','historical_archives':'preserved'},ensure_ascii=False,indent=2)+'\n')
print('Applied current assets to',len(changed),'web targets;',sum(i['changed'] for i in changed),'changed.')

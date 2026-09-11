# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Capture original HTML pages using the React workbench's shared fixture adapter."""
import argparse,hashlib,json
from pathlib import Path
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[2]

def main():
 p=argparse.ArgumentParser();p.add_argument('output',type=Path);p.add_argument('--base',default='http://127.0.0.1:49186');p.add_argument('--dev-reference',action='store_true');args=p.parse_args()
 manifest_path=args.output/'manifest.json';manifest=json.loads(manifest_path.read_text());folder=args.output/'reference';folder.mkdir(exist_ok=True)
 fixture=json.loads((ROOT/'crates/zork-ui/assets/stories/page-fixture.json').read_text());providers=json.loads((ROOT/'crates/zork-gui/tests/fixtures/provider_catalog.json').read_text())['providers']
 base=args.base.rstrip('/')+'/design/';entry=base+('src/bridge/reference-entry.ts' if args.dev_reference else 'reference-entry.js')
 source=ROOT/'apps/zork-design/src/reference'
 manifest['design_source']={'url':base+'reference.html','version':'2026-09-07 approved desktop controls','fixture_sha256':hashlib.sha256(json.dumps(fixture,sort_keys=True).encode()).hexdigest(),'files':{str(path.relative_to(source)):hashlib.sha256(path.read_bytes()).hexdigest() for path in source.rglob('*') if path.is_file()}}

 with sync_playwright() as pw:
  candidates=list((Path.home()/'Library/Caches/ms-playwright').glob('chromium-*/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'))
  browser=pw.chromium.launch(headless=True,executable_path=str(max(candidates,key=lambda path:path.parts[-6])) if candidates else None)
  for story in manifest['stories']:
   if story['family'] not in ['connection','model','agent','conversation','client','device','mesh','enrollment']:story['design']={'status':'not_applicable'};continue
   page=browser.new_page(viewport={'width':int(story['width']),'height':int(story['height'])},device_scale_factor=1)
   try:
    page.goto(base+'reference.html?isolate=1&story='+story['id'],wait_until='domcontentloaded')
    page.wait_for_function('(id)=>document.documentElement.dataset.referenceReady===id',arg=story['id'],timeout=30000)
    if page.locator('.ref-header-brand').count():page.locator('.ref-header-brand svg').wait_for()
    target=page.locator('[data-reference-target]').first;bounds=page.evaluate('window.zorkReference.snapshot().bounds');assert bounds and bounds['width']>0
    styles=target.evaluate("e=>{const s=getComputedStyle(e);return Object.fromEntries(['fontSize','fontFamily','fontWeight','lineHeight','color','backgroundColor','borderRadius','borderWidth','padding','gap'].map(k=>[k,s[k]]))}")
    full=folder/(story['id']+'-window.png');image=folder/(story['id']+'.png');page.screenshot(path=str(full));page.screenshot(path=str(image),clip=bounds)
    story['design']={'status':'captured','image':'reference/'+image.name,'window':'reference/'+full.name,'bounds':bounds,'styles':styles,'source':page.url,'fixture':'zork-ui/assets/stories/page-fixture.json; same viewport, fonts, identities and model values'}
    print('reference',story['id'],flush=True)
   finally:page.close()
  browser.close()
 manifest_path.write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
if __name__=='__main__':main()

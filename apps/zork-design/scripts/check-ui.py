# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Exercise the React workbench and save every paired page for visual review."""
import argparse,json
from pathlib import Path
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--base',default='http://127.0.0.1:49186/design/');p.add_argument('--output',type=Path,default=ROOT.parents[1]/'artifacts/storybook/react-qa');args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
with sync_playwright() as pw:
 binary=Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'
 browser=pw.chromium.launch(headless=True,executable_path=str(binary));page=browser.new_page(viewport={'width':1600,'height':1100},device_scale_factor=1);errors=[];loads=[]
 page.on('pageerror',lambda e:errors.append(str(e).splitlines()[0]));page.on('request',lambda r:loads.append(r.url) if '.wasm.gz' in r.url else None)
 page.goto(args.base+'#/pc/button');page.locator('[data-testid=workbench]').wait_for()
 def frame():return next(f for f in page.frames if '/components/web/index.html' in f.url)
 def ready(id):
  frame().wait_for_function('(id)=>window.zorkStory&&JSON.parse(zorkStory.story_state())?.id===id&&!JSON.parse(zorkStory.story_state()).pending_actions',arg=id,timeout=60000)
  assert not frame().evaluate('JSON.parse(zorkStory.story_state()).action_error'),frame().evaluate('zorkStory.story_state()')
  page.wait_for_timeout(250)
  assert not page.locator('[role=alert]').count(),page.locator('[role=alert]').all_text_contents()
  assert not errors,errors
 def route(path):page.evaluate('(path)=>location.hash=path',path)
 catalog=page.request.get(args.base+'catalog.json').json();families=list(dict.fromkeys(s['family'] for s in catalog['stories']));pages={'connection','model','agent','conversation','client','device','mesh','enrollment'};checked=[]
 for family in families:
  if family in pages:continue
  route('/pc/'+family);ready('family-'+family)
  snapshot=frame().evaluate('JSON.parse(zorkStory.snapshot())');assert snapshot['elements'],family
  page.locator('.component-content').screenshot(path=str(args.output/(family+'.png')))
  checked.append('family-'+family);print('PASS',checked[-1],flush=True)
 for story in catalog['stories']:
  if story['family'] not in pages:continue
  scene,size=story['state'].rsplit('-',1);route('/pc/'+story['family']+'/'+scene+'/'+size)
  page.wait_for_function('(id)=>document.querySelector(".workbench")?.dataset.family===id',arg=story['family'])
  if story['family']!='conversation':ready(story['id'])
  if story['design']['status']=='captured':
   page.wait_for_function('(id)=>{const f=document.querySelector(".reference-canvas-host iframe");return f?.contentDocument?.documentElement.dataset.referenceReady===id}',arg=story['id'],timeout=30000)
   f=next(f for f in page.frames if '/reference.html' in f.url)
   data=json.loads(f.evaluate('document.documentElement.dataset.fixture'));assert data['profile']==catalog['fixture']['profile']['profile_id'];assert data['width']==story['width'] and data['height']==story['height']
  page.wait_for_timeout(300)
  assert not page.locator('[role=alert]').count(),page.locator('[role=alert]').all_text_contents();assert not errors,errors
  page.locator('.component-content').screenshot(path=str(args.output/(story['id']+'.png')))
  checked.append(story['id']);print('PASS',story['id'],flush=True)
 # The engine survives both levels of navigation, retaining only one WASM instance.
 for outer in ['brand','product','mobile','materials']:
  route('/'+outer);page.locator('.handbook-page').wait_for(state='visible');page.wait_for_timeout(300);page.screenshot(path=str(args.output/('tab-'+outer+'.png')))
 route('/pc/dropdown');ready('family-dropdown');assert len(loads)==1,loads
 page.set_viewport_size({'width':900,'height':850});route('/pc/model/detail/compact');ready('model-detail-compact');page.wait_for_timeout(500);page.screenshot(path=str(args.output/'responsive-900.png'))
 for mode in ['并排快照','叠加','像素差异','实时对照']:
  page.get_by_role('button',name=mode,exact=True).click();page.wait_for_timeout(200);assert not errors,errors
 page.get_by_role('button',name='重置两侧').click();ready('model-detail-compact')
 result={'checked':checked,'outer_tabs':5,'wasm_loads':len(loads),'errors':errors};(args.output/'result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');browser.close()

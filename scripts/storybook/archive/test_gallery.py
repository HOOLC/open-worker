# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Verify component/page tabs, grouped GPUI states and live page comparisons."""
import argparse,json
from pathlib import Path
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[2]

def main():
 p=argparse.ArgumentParser();p.add_argument('--url',default='http://127.0.0.1:49174/design/components/index.html');args=p.parse_args()
 out=ROOT/'artifacts/storybook/gallery-checks';out.mkdir(parents=True,exist_ok=True)
 with sync_playwright() as pw:
  binary=Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'
  browser=pw.chromium.launch(headless=True,executable_path=str(binary));page=browser.new_page(viewport={'width':1440,'height':1000});errors=[];loads=[]
  page.on('pageerror',lambda e:errors.append(str(e)));page.on('request',lambda r:loads.append(r.url) if '.wasm.gz' in r.url else None)
  page.goto(args.url+'#button');page.wait_for_function('document.querySelector("#live-status").textContent.includes("可交互")',timeout=60000)
  frame=next(f for f in page.frames if '/components/web/' in f.url)
  def state():return frame.evaluate('JSON.parse(window.zorkStory.story_state())')
  def snapshot():return frame.evaluate('JSON.parse(window.zorkStory.snapshot())')
  def choose(family):
   page.locator(f'[role=tab][data-id="{family}"]').click()
   if family!='conversation':frame.wait_for_function('(id)=>JSON.parse(window.zorkStory.story_state()).id===id',arg=page.evaluate('requestedId()'))
   frame.evaluate('()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))')
  def click(id):
   frame.wait_for_function('(id)=>JSON.parse(window.zorkStory.snapshot()).elements.some(e=>e.id===id&&e.visible)',arg=id)
   e=next(e for e in snapshot()['elements'] if e['id']==id and e['visible'])
   box=page.locator('#live-native iframe').bounding_box();width=float(page.locator('#live-native iframe').get_attribute('width'));z=box['width']/width
   page.mouse.click(box['x']+e['center']['x']*z,box['y']+e['center']['y']*z)
   frame.evaluate('()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))')
  catalog=page.evaluate('data.stories');basics=list(dict.fromkeys(s['family'] for s in catalog if s['family'] not in ['connection','model','agent','conversation']))
  assert page.locator('[role=tab]').count()==len(basics)+4==16
  assert state()['id']=='family-button' and len(state()['states'])==5
  assert not any('/gui.html?' in f.url for f in page.frames),'Basic component loaded HTML reference'
  assert page.locator('#toolbar').is_hidden() and page.locator('#page-controls').is_hidden()
  click('button-primary-story-button');frame.wait_for_function('JSON.parse(window.zorkStory.story_state()).states.find(s=>s.id==="button-primary").clicks===1')
  click('button-disabled-story-button');assert next(s for s in state()['states'] if s['id']=='button-disabled')['clicks']==0
  page.screenshot(path=str(out/'buttons.png'),full_page=True)
  page.locator('#live-reset').click();frame.wait_for_function('JSON.parse(window.zorkStory.story_state()).states.every(s=>s.clicks===0)')
  choose('field');click('field-empty-story-field');ime=page.context.new_cdp_session(page);value='独立状态';end=len(value.encode('utf-16-le'))//2
  ime.send('Input.imeSetComposition',{'text':value,'selectionStart':end,'selectionEnd':end});ime.send('Input.insertText',{'text':value})
  frame.wait_for_function('(value)=>JSON.parse(window.zorkStory.story_state()).states.find(s=>s.id==="field-empty").text===value',arg=value)
  assert next(s for s in state()['states'] if s['id']=='field-value')['text']=='产品模型连接'
  assert next(s for s in state()['states'] if s['id']=='field-secret')['text']=='[redacted]'
  page.screenshot(path=str(out/'fields.png'),full_page=True)
  choose('dropdown');click('dropdown-closed-story-select');click('dropdown-closed-story-option-1')
  frame.wait_for_function('JSON.parse(window.zorkStory.story_state()).states.find(s=>s.id==="dropdown-closed").selected===1')
  for family in basics:
   choose(family);expected=[s for s in catalog if s['family']==family]
   assert len(state()['states'])==len(expected),(family,'missing states')
   assert not any('/gui.html?' in f.url for f in page.frames)
   assert len(loads)==1,'Changing tabs reloaded the WASM module'
   page.screenshot(path=str(out/('component-'+family+'.png')),full_page=True)
  choose('markdown');box=page.locator('#live-native iframe').bounding_box();page.mouse.move(box['x']+box['width']/2,box['y']+box['height']/2);page.mouse.wheel(0,700)
  frame.wait_for_function('JSON.parse(window.zorkStory.snapshot()).elements.some(e=>e.visible&&e.label==="已检查"&&e.id.endsWith("-selection"))')
  cells=[e for e in snapshot()['elements'] if e['visible'] and e['id'].endswith('-selection') and 'markdown-table' in e['id']]
  for cell in cells:click(cell['id'])
  cell=next(e for e in snapshot()['elements'] if e['visible'] and e['label']=='已检查' and e['id'].endswith('-selection'));bounds=cell['bounds'];box=page.locator('#live-native iframe').bounding_box()
  page.mouse.move(box['x']+bounds['x']+1,box['y']+cell['center']['y']);page.mouse.down();page.mouse.move(box['x']+bounds['x']+bounds['width']-1,box['y']+cell['center']['y'],steps=8);page.mouse.up()
  frame.wait_for_function('JSON.parse(window.zorkStory.story_state()).states.find(s=>s.id==="markdown-table").quote?.includes("已检查")')
  choose('connection');page.locator('#scene').select_option('create');page.locator('#page-size').select_option('wide')
  frame.wait_for_function('JSON.parse(window.zorkStory.story_state()).id==="connection-create-wide"')
  reference=next(f for f in page.frames if '/gui.html?' in f.url);reference.wait_for_function('document.documentElement.dataset.referenceReady==="connection-create-wide"')
  reference.locator('#profile-provider-menu summary').click();reference.locator('[data-action="profile-provider"][data-provider="anthropic"]').click();assert 'Anthropic' in reference.locator('#profile-provider-menu summary').inner_text()
  click('profile-provider-select');option=next(e['id'] for e in snapshot()['elements'] if e['id'].startswith('provider-option-') and e['label'].startswith('Anthropic'));click(option)
  page.screenshot(path=str(out/'page-comparison.png'),full_page=True)
  page.locator('[data-mode="overlay"]').click();page.locator('.overlay-stage').wait_for()
  page.locator('[data-mode="diff"]').click();page.locator('#overlay-pane canvas').wait_for()
  page.locator('[data-mode="side"]').click();assert page.locator('#comparison').is_visible()
  page.locator('[data-mode="live"]').click();choose('conversation');assert page.locator('#live-status').inner_text()=='整页原生对照'
  choose('button');assert len(state()['states'])==5 and len(loads)==1
  page.locator('#search').fill('输入框');assert page.locator('[role=tab]').count()==1;page.locator('#search').fill('')
  page.set_viewport_size({'width':900,'height':900});page.wait_for_timeout(150);assert page.locator('#overlay-pane').is_hidden() and page.locator('#comparison').is_hidden(),'Resize exposed a snapshot over the live canvas';choose('field');page.screenshot(path=str(out/'compact-components.png'),full_page=True)
  choose('agent');page.locator('#scene').select_option('create');page.locator('#page-size').select_option('compact');frame.wait_for_function('JSON.parse(window.zorkStory.story_state()).id==="agent-create-compact"');frame.wait_for_function('JSON.parse(window.zorkStory.snapshot()).elements.some(e=>e.id==="agent-create-dialog")');frame.evaluate('()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');assert page.locator('#overlay-pane').is_hidden();page.screenshot(path=str(out/'compact-page.png'),full_page=True)
  checked=[]
  for story in catalog:
   if story['family'] not in ['connection','model','agent','conversation']:continue
   if story['design']['status']!='captured':continue
   choose(story['family']);page.locator('#scene').select_option(story['state'].rsplit('-',1)[0]);page.locator('#page-size').select_option(story['state'].rsplit('-',1)[1])
   reference=next(f for f in page.frames if '/gui.html?' in f.url);reference.wait_for_function('(id)=>document.documentElement.dataset.referenceReady===id',arg=story['id'],timeout=15000)
   assert reference.locator('[data-reference-target]').count(),story['id'];checked.append(story['id'])
  assert not errors,errors
  (out/'result.json').write_text(json.dumps({'component_tabs':len(basics),'page_tabs':4,'checks':['all primitive states inside one GPUI tab','no primitive HTML references','independent input state','table clicks and drag selection in grouped states','disabled button','page state/size inside page tab','original HTML page interaction','side/overlay/pixel difference','single WASM instance across tabs','compact layouts'],'page_html_states':checked,'wasm_loads':len(loads),'errors':errors},ensure_ascii=False,indent=2)+'\n')
  browser.close();print('PASS 12 component tabs, 4 page tabs, grouped states and page comparisons')
if __name__=='__main__':main()

# /// script
# dependencies = ["playwright==1.58.0"]
# ///
from pathlib import Path
import argparse,json
from playwright.sync_api import sync_playwright
parser=argparse.ArgumentParser();parser.add_argument('--base',default='http://127.0.0.1:49186/design/');args=parser.parse_args()
out=Path(__file__).resolve().parents[2]/'artifacts/storybook/tooltip-checks';out.mkdir(parents=True,exist_ok=True)
with sync_playwright() as p:
 b=p.chromium.launch(headless=True,executable_path=str(Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'));page=b.new_page(viewport={'width':640,'height':400});errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
 page.goto(args.base+'components/web/index.html?story=tooltip-hover');page.wait_for_function('document.documentElement.dataset.ready==="true"',timeout=60000)
 def els():return page.evaluate('JSON.parse(zorkStory.snapshot()).elements')
 def cards():return [e for e in els() if e['id'].startswith('detail-tooltip-') and e['visible']]
 assert not cards(),cards()
 for suffix,kind in [('leader','Leader'),('task','Task')]:
  e=next(e for e in els() if e['id'].endswith('tooltip-'+suffix+'-trigger'));page.mouse.move(e['center']['x'],e['center']['y']);page.wait_for_timeout(50);c=cards();assert len(c)==1 and kind in c[0]['label'],c
  r=c[0]['bounds'];assert abs(r['x']-e['bounds']['x']-e['bounds']['width']-4)<.1,(e,r);assert r['x']>=0 and r['y']>=0 and r['x']+r['width']<=640 and r['y']+r['height']<=400,r
  page.mouse.move(r['x']+r['width']/2,r['y']+r['height']/2);page.wait_for_timeout(650);assert cards();page.screenshot(path=str(out/(suffix+'-hover.png')))
  page.mouse.move(630,390);page.wait_for_timeout(40);page.mouse.move(r['x']+r['width']/2,r['y']+r['height']/2);page.wait_for_timeout(300);assert cards();page.mouse.move(630,390);page.wait_for_timeout(250);assert not cards()
 # One shared surface must survive retargeting, including mid-flight reversals.
 leader=next(e for e in els() if e['id'].endswith('tooltip-leader-trigger'))
 task=next(e for e in els() if e['id'].endswith('tooltip-task-trigger'))
 page.mouse.move(leader['center']['x'],leader['center']['y']);page.wait_for_timeout(50)
 assert len(cards())==1 and abs(cards()[0]['bounds']['y']-leader['bounds']['y'])<.1
 initial_height=cards()[0]['bounds']['height']
 page.mouse.move(task['center']['x'],task['center']['y']);page.wait_for_timeout(40)
 moving=cards();assert len(moving)==1 and 'Task' in moving[0]['label'],moving
 assert leader['bounds']['y']<moving[0]['bounds']['y']<task['bounds']['y'],moving
 page.screenshot(path=str(out/'shared-moving.png'))
 for target in [leader,task,leader,task]:
  page.mouse.move(target['center']['x'],target['center']['y']);page.wait_for_timeout(20)
  assert len(cards())==1,cards()
 page.wait_for_timeout(450);assert abs(cards()[0]['bounds']['y']-task['bounds']['y'])<.1,cards()
 final_height=cards()[0]['bounds']['height']
 assert initial_height < moving[0]['bounds']['height'] < final_height,(initial_height,moving,final_height)
 page.screenshot(path=str(out/'shared-settled.png'))
 page.mouse.move(630,390);page.wait_for_timeout(130);assert len(cards())==1,'card should remain during fade-out'
 page.mouse.move(task['center']['x'],task['center']['y']);page.wait_for_timeout(200);assert len(cards())==1,'re-entry must cancel fade-out'
 page.emulate_media(reduced_motion='reduce');page.wait_for_timeout(80)
 page.mouse.move(leader['center']['x'],leader['center']['y']);page.wait_for_timeout(50)
 assert len(cards())==1 and abs(cards()[0]['bounds']['y']-leader['bounds']['y'])<.1,cards()
 page.emulate_media(reduced_motion='no-preference')
 page.mouse.move(630,390);page.wait_for_timeout(250);assert not cards()
 page.set_viewport_size({'width':560,'height':400});page.wait_for_timeout(150)
 e=next(e for e in els() if e['id'].endswith('tooltip-leader-trigger'));page.mouse.move(e['center']['x'],e['center']['y']);page.wait_for_timeout(550);c=cards();assert len(c)==1;c=c[0]['bounds'];assert c['x']>=e['bounds']['x']+e['bounds']['width']+4 and c['x']+c['width']<=548,c;page.screenshot(path=str(out/'right-narrow.png'));page.mouse.move(550,390);page.wait_for_timeout(250)
 page.set_viewport_size({'width':900,'height':600});page.goto(args.base+'reference.html?story=conversation-messages-compact');page.wait_for_function('document.documentElement.dataset.referenceReady');page.wait_for_timeout(200)
 trigger=page.get_by_role('button',name='产品 Leader',exact=True);trigger.hover();page.wait_for_timeout(450);tip=page.get_by_role('tooltip');assert tip.is_visible();assert 'fixture-model' in tip.inner_text();tip.hover();page.wait_for_timeout(550);assert tip.is_visible();page.screenshot(path=str(out/'reference-leader-hover.png'));page.keyboard.press('Escape');page.wait_for_timeout(450);assert tip.count()==0
 page.mouse.move(899,599);trigger.hover();page.wait_for_timeout(450);assert tip.is_visible();page.mouse.move(899,599);page.wait_for_timeout(250);assert tip.count()==0
 assert page.get_by_text('Enter 发送',exact=False).count()==0
 send=page.get_by_role('button',name='发送',exact=True);assert send.is_disabled()
 editor=page.locator('textarea');editor.fill('tooltip regression');assert send.is_enabled();editor.press('Enter');page.wait_for_timeout(150);assert editor.input_value()=='';assert send.is_disabled()
 (out/'result.json').write_text(json.dumps({'native_wasm':'Leader/Task delay, content, enter-card, leave, viewport bounds','reference':'Leader hover, enter-card, Escape, leave, removed hint, disabled send, Enter send','errors':errors},ensure_ascii=False,indent=2))
 b.close();assert not errors,errors
 print('PASS tooltip interactions and conversation send')

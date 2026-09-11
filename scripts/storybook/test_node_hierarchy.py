# /// script
# dependencies = ["playwright==1.58.0"]
# ///
from pathlib import Path
import json
from playwright.sync_api import sync_playwright
out=Path(__file__).resolve().parents[2]/'artifacts/storybook/node-hierarchy';out.mkdir(parents=True,exist_ok=True)
base='http://127.0.0.1:49186/design/'
with sync_playwright() as pw:
 b=pw.chromium.launch(headless=True,executable_path=str(Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'));page=b.new_page();errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
 for width,height,size in [(900,600,'compact'),(1280,800,'wide')]:
  page.set_viewport_size({'width':width,'height':height});page.goto(base+'reference.html?story=device-running-'+size);page.wait_for_function('document.documentElement.dataset.referenceReady')
  page.screenshot(path=str(out/('reference-'+size+'.png')))
  status=page.locator('.ref-device-runtime').bounding_box();mode=page.locator('.ref-device-mode').bounding_box();version=page.locator('.ref-device-version').bounding_box();assert status['y']+status['height']<mode['y']<version['y'];assert version['y']+version['height']<height
  page.goto(base+'components/web/index.html?story=device-running-'+size);page.wait_for_function('document.documentElement.dataset.ready==="true"');page.wait_for_timeout(300)
  def elements():return page.evaluate('JSON.parse(zorkStory.snapshot()).elements')
  def element(id):return next(e for e in elements() if e['id']==id and e['visible'])
  def click(id):
   e=element(id);page.mouse.click(e['center']['x'],e['center']['y']);page.wait_for_timeout(60)
  page.screenshot(path=str(out/('gpui-'+size+'.png')));assert element('device-check-update')['center']['y']>element('local-node-background')['center']['y']>element('local-node-toggle')['center']['y']
  click('local-node-login');assert element('local-node-login')['label'].endswith('开启')
  click('local-node-foreground');assert not any(e['id'] in ['local-node-login','device-check-update','device-upgrade'] for e in elements())
  page.keyboard.press('ArrowRight');page.wait_for_timeout(60);assert element('local-node-login')['label'].endswith('关闭')
  page.keyboard.press('Home');page.wait_for_timeout(60);assert not any(e['id'] in ['local-node-login','device-check-update','device-upgrade'] for e in elements())
  page.keyboard.press('Enter');page.wait_for_timeout(60);assert not any(e['id'] in ['local-node-login','device-check-update','device-upgrade'] for e in elements())
  click('local-node-background');page.keyboard.press('ArrowLeft');page.wait_for_timeout(60);assert not any(e['id'] in ['local-node-login','device-check-update','device-upgrade'] for e in elements())
  page.keyboard.press('Space');page.wait_for_timeout(60);assert element('local-node-login')['label'].endswith('关闭')
  click('device-check-update');assert element('device-upgrade')['enabled'];click('device-upgrade');assert not any(e['id']=='device-upgrade' for e in elements())
  for state in ['stopped','loading','error']:
   page.evaluate('(id)=>zorkStory.select_story(id)','device-'+state+'-'+size);page.wait_for_timeout(250);page.screenshot(path=str(out/(state+'-'+size+'.png')))
   if state=='loading':assert not element('local-node-background')['enabled'] and not element('local-node-toggle')['enabled']
 assert not errors,errors
 (out/'result.json').write_text(json.dumps({'viewports':[[900,600],[1280,800]],'hierarchy':True,'mode_click_and_keyboard':True,'login_reset':True,'update_flow':True,'states':['running','stopped','loading','error'],'errors':errors},indent=2));print('PASS node hierarchy, modes, keyboard, login dependency and version controls');b.close()

# /// script
# dependencies = ["playwright==1.58.0", "pillow==11.3.0"]
# ///
"""Foreground clarity, dark compositor scrim, and overlay lifetime."""
from pathlib import Path
import argparse,io,json
from PIL import Image,ImageChops
from playwright.sync_api import sync_playwright
p=argparse.ArgumentParser();p.add_argument('--base',default='http://127.0.0.1:49186/design/');args=p.parse_args();out=Path(__file__).resolve().parents[2]/'artifacts/storybook/modal-material-checks';out.mkdir(parents=True,exist_ok=True)
with sync_playwright() as pw:
 binary=Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing';browser=pw.chromium.launch(headless=True,executable_path=str(binary));page=browser.new_page(viewport={'width':900,'height':700});errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
 page.goto(args.base+'components/web/index.html?story=connection-create-compact');page.wait_for_function('document.documentElement.dataset.ready==="true"',timeout=60000)
 layer=page.locator('[data-zork-modal-blur]');layer.wait_for();page.wait_for_timeout(250);assert layer.count()==1
 style=layer.evaluate('(e)=>({blur:getComputedStyle(e).backdropFilter,background:getComputedStyle(e).backgroundColor,pointer:getComputedStyle(e).pointerEvents})');assert style=={'blur':'none','background':'rgba(0, 0, 0, 0.35)','pointer':'none'},style
 def elements():return page.evaluate('JSON.parse(zorkStory.snapshot()).elements')
 card=next(e for e in elements() if e['role']=='status' and e['id'].endswith('-dialog') and e['visible']);r=card['bounds'];blurred=Image.open(io.BytesIO(page.screenshot())).convert('RGB');blurred.save(out/'web-dark-scrim.png');assert min(blurred.getpixel((round(r['x']+r['width']/2),round(r['y']+1))))>250,'modal has an outline'
 layer.evaluate('(e)=>{e.style.backdropFilter="none";e.style.webkitBackdropFilter="none"}');page.wait_for_timeout(100);plain=Image.open(io.BytesIO(page.screenshot())).convert('RGB');inside=(round(r['x']+12),round(r['y']+12),round(r['x']+r['width']-12),round(r['y']+64));assert ImageChops.difference(blurred.crop(inside),plain.crop(inside)).getbbox() is None,'foreground was blurred';assert ImageChops.difference(blurred.crop((0,0,900,80)),plain.crop((0,0,900,80))).getbbox() is None,'background must not be filtered'
 page.keyboard.press('Escape');page.wait_for_timeout(250);assert layer.count()==0
 for _ in range(3):
  page.evaluate('zorkStory.select_story("connection-create-compact")');layer.wait_for();page.wait_for_timeout(100);assert layer.count()==1;page.keyboard.press('Escape');page.wait_for_timeout(250);assert layer.count()==0
 page.goto(args.base+'reference.html?story=connection-create-compact');page.wait_for_function('document.documentElement.dataset.referenceReady');page.wait_for_timeout(250);dialog=page.locator('.ref-modal');spec=dialog.evaluate('(e)=>({border:getComputedStyle(e).borderTopWidth,background:getComputedStyle(e,"::backdrop").backgroundColor,blur:getComputedStyle(e,"::backdrop").backdropFilter})');assert spec['border']=='0px' and spec['blur']=='none' and spec['background']=='rgba(0, 0, 0, 0)',spec;assert page.locator('.ref-modal-scrim').evaluate('(e)=>getComputedStyle(e).backgroundColor')=='rgba(0, 0, 0, 0.55)';assert not errors,errors
 (out/'web-result.json').write_text(json.dumps({'compositor':style,'reference':spec,'foreground_unchanged':True,'reopen_cycles':3,'errors':errors},indent=2));browser.close();print('PASS dark scrim without blur, borderless sharp foreground, close/reopen cleanup')

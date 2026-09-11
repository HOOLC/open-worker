# /// script
# dependencies = ["playwright==1.58.0"]
# ///
from pathlib import Path
import argparse,json
from playwright.sync_api import sync_playwright
parser=argparse.ArgumentParser();parser.add_argument('--base',default='http://127.0.0.1:49186/design/');args=parser.parse_args()
out=Path(__file__).resolve().parents[2]/'artifacts/storybook/header-checks';out.mkdir(parents=True,exist_ok=True)
with sync_playwright() as p:
 b=p.chromium.launch(headless=True,executable_path=str(Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'));page=b.new_page(viewport={'width':640,'height':400});errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
 page.goto(args.base+'components/web/index.html?story=brand-header');page.wait_for_function('document.documentElement.dataset.ready==="true"',timeout=60000)
 def progress():return page.evaluate('JSON.parse(zorkStory.story_state()).brand_progress')
 es=page.evaluate('JSON.parse(zorkStory.snapshot()).elements');r=next(e for e in es if e['id']=='brand-header')['bounds'];assert progress()==1
 page.screenshot(path=str(out/'wordmark.png'));page.mouse.move(r['x']+r['width']/2,r['y']+r['height']/2);page.wait_for_timeout(400);a=progress();assert .5<a<.95,a
 page.mouse.move(630,390);b0=progress();assert abs(b0-a)<.04,(a,b0);page.wait_for_timeout(150);c=progress();assert b0<c<1,(b0,c)
 page.mouse.move(r['x']+r['width']/2,r['y']+r['height']/2);d=progress();assert abs(d-c)<.04,(c,d);page.wait_for_timeout(150);e=progress();assert e<d,(e,d);page.screenshot(path=str(out/'reverse-interrupted.png'));page.wait_for_timeout(2100);assert progress()==0;page.screenshot(path=str(out/'icon.png'));page.mouse.move(630,390);page.wait_for_timeout(2100);assert progress()==1
 page.set_viewport_size({'width':900,'height':600});page.goto(args.base+'reference.html?story=conversation-messages-compact');page.wait_for_function('document.documentElement.dataset.referenceReady');brand=page.locator('.ref-header-brand');brand.locator('svg').wait_for();assert page.locator('.ref-chrome').count()==0;assert brand.bounding_box()['y']==0,brand.bounding_box();page.screenshot(path=str(out/'reference-header.png'));brand.hover();page.wait_for_timeout(400);rp=float(brand.get_attribute('data-progress'));assert .5<rp<.95,rp;page.mouse.move(850,550);page.wait_for_timeout(150);rp2=float(brand.get_attribute('data-progress'));assert rp<rp2<1,(rp,rp2)
 assert not errors,errors;(out/'result.json').write_text(json.dumps({'forward':a,'reverse_start':b0,'reverse_mid':c,'forward_again_start':d,'forward_again_mid':e,'reference':[rp,rp2],'errors':errors},indent=2));print('PASS default wordmark, bidirectional SVG continuity, header row');b.close()

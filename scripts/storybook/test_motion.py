# /// script
# dependencies = ["playwright==1.58.0", "pillow==11.3.0"]
# ///
from pathlib import Path
import argparse,io,json
from PIL import Image
from playwright.sync_api import sync_playwright
parser=argparse.ArgumentParser();parser.add_argument('--base',default='http://127.0.0.1:49186/design/');args=parser.parse_args()
out=Path(__file__).resolve().parents[2]/'artifacts/storybook/motion-checks';out.mkdir(parents=True,exist_ok=True)
with sync_playwright() as p:
 b=p.chromium.launch(headless=True,executable_path=str(Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'));page=b.new_page(viewport={'width':640,'height':400});errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
 page.goto(args.base+'components/web/index.html?story=choice-selected');page.wait_for_function('document.documentElement.dataset.ready==="true"',timeout=60000);page.wait_for_timeout(300)
 def el(id):return next(e for e in page.evaluate('JSON.parse(zorkStory.snapshot()).elements') if e['id']==id and e['visible'])
 a=el('story-choice-0')['bounds'];c=el('story-choice-1')['bounds']
 def snap(name):
  im=Image.open(io.BytesIO(page.screenshot())).convert('RGB');im.save(out/(name+'.png'));y=round(a['y']+5);white=[x for x in range(round(a['x']),round(c['x']+c['width'])) if min(im.getpixel((x,y)))>250];return sum(white)/len(white) if white else None
 positions=[snap('choice-before')];page.mouse.click(c['x']+c['width']/2,c['y']+16);page.wait_for_timeout(40);positions.append(snap('choice-during'));page.wait_for_timeout(350);positions.append(snap('choice-after'));print('positions',positions);assert positions[0]<positions[1]<positions[2],positions
 page.emulate_media(reduced_motion='reduce');page.wait_for_timeout(60);page.mouse.click(a['x']+a['width']/2,a['y']+16);positions.append(snap('choice-reduced'));assert abs(positions[-1]-positions[0])<1,positions
 page.emulate_media(reduced_motion='no-preference');page.wait_for_timeout(60)
 for _ in range(3):
  page.mouse.click(c['x']+c['width']/2,c['y']+16);page.wait_for_timeout(35);page.mouse.click(a['x']+a['width']/2,a['y']+16);page.wait_for_timeout(35)
 page.wait_for_timeout(350);reversed_center=snap('choice-interrupted');assert abs(reversed_center-positions[0])<1,reversed_center
 page.evaluate('zorkStory.select_story("switch-off")');page.wait_for_function('JSON.parse(zorkStory.story_state()).id==="switch-off"');page.wait_for_timeout(200);r=el('story-switch')['bounds']
 def thumb(name):
  im=Image.open(io.BytesIO(page.screenshot())).convert('RGB');im.save(out/(name+'.png'));white=[x for x in range(round(r['x']+4),round(r['x']+44)) if min(im.getpixel((x,round(r['y']+16))))>250];return sum(white)/len(white)
 thumbs=[thumb('switch-before')];page.mouse.click(r['x']+24,r['y']+16);page.wait_for_timeout(40);thumbs.append(thumb('switch-during'));page.wait_for_timeout(350);thumbs.append(thumb('switch-after'));assert thumbs[0]<thumbs[1]<thumbs[2],thumbs
 page.evaluate('zorkStory.select_story("modal-standard")');page.wait_for_function('JSON.parse(zorkStory.snapshot()).elements.some(e=>e.id==="story-modal"&&e.visible)');start=el('story-modal')['bounds']['y'];page.screenshot(path=str(out/'modal-enter.png'));page.wait_for_timeout(250);end=el('story-modal')['bounds']['y'];assert start>=end,(start,end)
 page.emulate_media(reduced_motion='reduce');page.evaluate('zorkStory.select_story("switch-off")');page.wait_for_timeout(100);page.evaluate('zorkStory.select_story("modal-standard")');page.wait_for_function('JSON.parse(zorkStory.snapshot()).elements.some(e=>e.id==="story-modal"&&e.visible)');reduced=el('story-modal')['bounds']['y'];assert abs(reduced-end)<.1,(reduced,end)
 assert not errors,errors;(out/'result.json').write_text(json.dumps({'choice_centers':positions,'switch_centers':thumbs,'rapid_reversal_center':reversed_center,'modal_y':[start,end,reduced],'reduced_motion':'immediate endpoint','errors':errors},indent=2));print('PASS continuous choice/switch motion, modal entrance and reduced motion');b.close()

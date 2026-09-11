# /// script
# dependencies = ["playwright==1.58.0", "pillow==11.3.0"]
# ///
"""Check rendered icon centering, segmented selection and composer insets."""
import argparse,io,json
from pathlib import Path
from PIL import Image
from playwright.sync_api import sync_playwright
parser=argparse.ArgumentParser();parser.add_argument('--base',default='http://127.0.0.1:49186/design/');args=parser.parse_args()
out=Path(__file__).resolve().parents[2]/'artifacts/storybook/alignment-checks';out.mkdir(parents=True,exist_ok=True)
with sync_playwright() as p:
 b=p.chromium.launch(headless=True,executable_path=str(Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'));page=b.new_page(viewport={'width':900,'height':600});errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
 page.goto(args.base+'components/web/index.html?story=modal-standard');page.wait_for_function('document.documentElement.dataset.ready==="true"',timeout=60000)
 def els():return page.evaluate('JSON.parse(zorkStory.snapshot()).elements')
 def el(id):return next(e for e in els() if e['id']==id and e['visible'])
 def choose(id):
  page.mouse.move(899,599);page.evaluate('(id)=>zorkStory.select_story(id)',id);page.wait_for_timeout(200)
 e=el('story-modal-close');page.mouse.move(e['center']['x'],e['center']['y']);page.wait_for_timeout(150);r=e['bounds'];im=Image.open(io.BytesIO(page.screenshot())).convert('RGB');im=im.crop((round(r['x']),round(r['y']),round(r['x']+r['width']),round(r['y']+r['height'])));im.save(out/'close-hover.png');ink=[(x,y) for y in range(im.height) for x in range(im.width) if max(im.getpixel((x,y)))<180];bb=[min(x for x,y in ink),min(y for x,y in ink),max(x for x,y in ink)+1,max(y for x,y in ink)+1];offset=[(bb[0]+bb[2]-im.width)/2,(bb[1]+bb[3]-im.height)/2];assert max(abs(v) for v in offset)<=.5,offset;assert im.getpixel((4,16))==(250,250,248)
 choose('choice-selected');a=el('story-choice-0')['bounds'];c=el('story-choice-1')['bounds'];assert abs(c['x']-a['x']-a['width']-2)<.1,(a,c);page.mouse.click(c['x']+c['width']/2,c['y']+c['height']/2);page.wait_for_timeout(150);assert page.evaluate('JSON.parse(zorkStory.story_state()).selected')==1;page.mouse.move(899,599);page.wait_for_timeout(100);page.screenshot(path=str(out/'segmented-choice.png'))
 im=Image.open(io.BytesIO(page.screenshot())).convert('RGB');assert im.getpixel((round(c['x']+8),round(c['y']+16)))==(255,255,255);assert im.getpixel((round(a['x']+8),round(a['y']+16)))==(245,245,245)
 page.goto(args.base+'reference.html?story=mesh-list-compact');page.wait_for_function('document.documentElement.dataset.referenceReady');button=page.get_by_role('button',name='手动连接',exact=True);padding=button.evaluate('(e)=>[getComputedStyle(e).paddingLeft,getComputedStyle(e).paddingRight]');assert padding==['12px','16px'],padding;button.screenshot(path=str(out/'leading-icon-button.png'))
 page.goto(args.base+'reference.html?story=conversation-composer-compact');page.wait_for_function('document.documentElement.dataset.referenceReady');geometry=page.locator('.ref-composer').evaluate('(e)=>({outer:getComputedStyle(e).padding,inner:getComputedStyle(e.querySelector("textarea")).padding})');assert geometry=={'outer':'12px','inner':'8px 8px 0px'},geometry;page.locator('.ref-composer').screenshot(path=str(out/'composer-insets.png'))
 assert not errors,errors;(out/'result.json').write_text(json.dumps({'close_center_offset':offset,'leading_icon_padding':padding,'composer':geometry,'segment_gap':2,'errors':errors},indent=2)+'\n');b.close();print('PASS centered close, selected capsule, leading icon padding, composer insets')

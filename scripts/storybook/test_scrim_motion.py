# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Exercise actual open/close and reversal through browser compositor frames."""
from pathlib import Path
import json
from playwright.sync_api import sync_playwright
out=Path(__file__).resolve().parents[2]/'artifacts/storybook/scrim-motion';out.mkdir(parents=True,exist_ok=True)
base='http://127.0.0.1:49186/design/'
with sync_playwright() as pw:
 b=pw.chromium.launch(headless=True,executable_path=str(Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'))
 page=b.new_page(viewport={'width':900,'height':700});errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
 page.goto(base+'components/web/index.html?story=button-primary');page.wait_for_function('document.documentElement.dataset.ready==="true"')
 page.evaluate('zorkStory.select_story("connection-create-compact")');layer=page.locator('[data-zork-modal-blur]');layer.wait_for(state='attached')
 def samples(selector,duration=260):
  return page.evaluate('''async ([selector,duration])=>{ const e=document.querySelector(selector), values=[], start=performance.now(); do {values.push(e?.isConnected?Number(getComputedStyle(e).opacity):0);await new Promise(requestAnimationFrame);}while(performance.now()-start<duration);return values;}''',[selector,duration])
 enter=samples('[data-zork-modal-blur]');assert sum(0<x<1 for x in enter)>=3,enter;assert enter[-1]>.99,enter
 page.keyboard.press('Escape');leave=samples('[data-zork-modal-blur]');assert sum(0<x<1 for x in leave)>=3,leave;assert leave[-1]==0,leave
 page.evaluate('zorkStory.select_story("connection-create-compact")');layer.wait_for();page.wait_for_timeout(250)
 page.evaluate('window.previousScrim=document.querySelector("[data-zork-modal-blur]")');page.keyboard.press('Escape');page.wait_for_timeout(40)
 before=page.evaluate('Number(getComputedStyle(window.previousScrim).opacity)');page.evaluate('zorkStory.select_story("connection-create-compact")');page.wait_for_timeout(40)
 assert page.evaluate('document.querySelector("[data-zork-modal-blur]")===window.previousScrim'),'reopen recreated scrim'
 after=layer.evaluate('(e)=>Number(getComputedStyle(e).opacity)');assert after>0 and before>0,(before,after)
 page.wait_for_timeout(260);assert layer.count()==1;page.keyboard.press('Escape');page.wait_for_timeout(240);assert layer.count()==0
 page.goto(base+'reference.html?story=connection-create-compact');page.locator('.ref-modal-scrim').wait_for();page.wait_for_timeout(250);page.keyboard.press('Escape');ref_leave=samples('.ref-modal-scrim');assert sum(0<x<1 for x in ref_leave)>=3,ref_leave;assert ref_leave[-1]==0
 page.emulate_media(reduced_motion='reduce');page.goto(base+'components/web/index.html?story=connection-create-compact');page.wait_for_function('document.documentElement.dataset.ready==="true"');layer.wait_for();page.wait_for_timeout(50);assert layer.evaluate('(e)=>getComputedStyle(e).opacity')=='1';page.keyboard.press('Escape');page.wait_for_timeout(80);assert layer.count()==0
 assert not errors,errors
 (out/'web-result.json').write_text(json.dumps({'enter':enter,'exit':leave,'reverse':[before,after],'reference_exit':ref_leave,'reduced_motion':True,'errors':errors},indent=2))
 print('PASS compositor entry/exit frames, reversal reuse, cleanup and reduced motion');b.close()

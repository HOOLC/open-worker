# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Regression: motion replay, cold page entry, lost ready event and engine recovery."""
import argparse,json,re
from pathlib import Path
from playwright.sync_api import sync_playwright
p=argparse.ArgumentParser();p.add_argument('--base',default='http://127.0.0.1:49186/design/');args=p.parse_args();out=Path(__file__).resolve().parents[3]/'artifacts/storybook/loading-motion';out.mkdir(parents=True,exist_ok=True)
with sync_playwright() as pw:
 binary=Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'
 browser=pw.chromium.launch(headless=True,executable_path=str(binary));page=browser.new_page(viewport={'width':1117,'height':837});errors=[];page.on('pageerror',lambda e:errors.append(str(e).splitlines()[0]))
 # Suppress the one-shot event: loading must still finish by checking the actual canvas.
 page.add_init_script('if(window===top)window.addEventListener("message",e=>{if(e.data?.type==="zork-story-ready")e.stopImmediatePropagation()},true)')
 page.goto(args.base+'#/brand');buttons=page.locator('.motion-preview');assert buttons.count()==4
 motion=[]
 for index in range(4):
  button=buttons.nth(index);obj=button.locator('object');button.scroll_into_view_if_needed();page.mouse.move(2,2)
  page.wait_for_function('(index)=>{const s=document.querySelectorAll(".motion-preview object")[index]?.contentDocument?.documentElement;return s?.animationsPaused()&&s.getCurrentTime()<.1}',arg=index)
  start=button.screenshot();button.hover();page.wait_for_timeout(240);assert obj.evaluate('e=>e.contentDocument.documentElement.getCurrentTime()')>.1
  assert button.screenshot()!=start
  page.mouse.move(2,2);page.wait_for_timeout(100);assert obj.evaluate('e=>e.contentDocument.documentElement.getCurrentTime()')<.1
  button.hover();page.wait_for_timeout(200);assert obj.evaluate('e=>e.contentDocument.documentElement.getCurrentTime()')>.1
  button.screenshot(path=str(out/f'motion-{index}.png'));motion.append(index)
 page.get_by_role('tab',name='PC 组件库',exact=True).click();page.get_by_role('tab',name='模型连接',exact=True).click()
 def ready():
  path=page.evaluate('location.hash').removeprefix('#/pc').strip('/').split('/')
  family=path[0] or 'button';scene=path[1] if len(path)>1 else ('create' if family=='model' else 'list');size=path[2] if len(path)>2 else 'compact';expected='family-'+family if family=='button' else '-'.join([family,scene,size])
  frame=next(f for f in page.frames if '/components/web/index.html' in f.url)
  frame.wait_for_function('(id)=>window.zorkStory&&JSON.parse(zorkStory.story_state())?.id===id&&!JSON.parse(zorkStory.story_state()).pending_actions',arg=expected,timeout=60000)
  page.locator('.gpui-pane .pane-heading span').filter(has_text=re.compile('^可交互$')).wait_for(timeout=60000);page.wait_for_timeout(150);assert not page.locator(".gpui-pane .canvas-message:visible").count();assert not page.locator('[role=alert]').count()
 ready();page.screenshot(path=str(out/'cold-connection.png'))
 for path in ['/brand','/pc/model/create/compact','/materials','/pc/connection','/pc/button','/pc/connection/create/wide','/pc/connection/list/compact']:
  page.evaluate('(path)=>location.hash=path',path)
  if path.startswith('/pc'):ready()
  else:page.wait_for_timeout(100)
 frame=next(f for f in page.frames if '/components/web/index.html' in f.url)
 frame.evaluate('parent.postMessage({type:"zork-story-error",message:"测试画布中断"},location.origin)')
 page.get_by_role('button',name='重新加载组件',exact=True).wait_for();assert page.locator('.gpui-pane .pane-heading').inner_text().endswith('加载失败')
 page.get_by_role('button',name='重新加载组件',exact=True).click();ready();page.screenshot(path=str(out/'recovered-connection.png'))
 # A direct cold deep link must also work, without ever opening a primitive first.
 page.goto(args.base+'#/pc/connection');ready()
 assert not errors,errors
 (out/'result.json').write_text(json.dumps({'hover_replays':motion,'ready_message_suppressed':True,'cold_connection_entry':True,'hidden_tab_return':True,'manual_engine_recovery':True,'direct_connection_entry':True,'errors':errors},indent=2)+'\n');browser.close();print('PASS motion replay, cold entry, lost ready event, tab return and engine recovery')

# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Browser navigation input against the production GPUI row, with offline screenshots."""
import argparse
import json
from pathlib import Path
from playwright.sync_api import sync_playwright

def main():
    p=argparse.ArgumentParser()
    p.add_argument('--url', default='http://127.0.0.1:49186/design/components/web/index.html')
    p.add_argument('--output', type=Path, default=Path('artifacts/ui-unification/navigation-web'))
    p.add_argument('--backend', choices=['auto','webgl'], default='auto')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    with sync_playwright() as pw:
        binary=Path.home()/'Library/Caches/ms-playwright/chromium-1228/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'
        options={'executable_path':str(binary)} if binary.exists() else {}
        browser=pw.chromium.launch(headless=True,**options)
        context=browser.new_context(viewport={'width':400,'height':360})
        page=context.new_page();errors=[]
        page.on('pageerror',lambda e:errors.append(str(e)))
        page.goto(args.url+'?story=navigation-default&backend='+args.backend)
        page.wait_for_function('document.documentElement.dataset.ready==="true"',timeout=60000)
        context.set_offline(True)
        def row():
            return page.wait_for_function('JSON.parse(window.zorkStory.snapshot()).elements.find(e=>e.id==="story-nav"&&e.visible)',timeout=10000).json_value()
        def state(expected):
            page.wait_for_function('(expected)=>JSON.parse(window.zorkStory.story_state()).state===expected',arg=expected)
        report=[]
        for width in [400,800]:
            page.set_viewport_size({'width':width,'height':360})
            for initial in ['default','selected']:
                page.mouse.move(0,0)
                page.evaluate('(id)=>window.zorkStory.select_story(id)','navigation-'+initial)
                state(initial);e=row();bounds=e['bounds']
                assert bounds['width']==240 and bounds['height']==32,bounds
                page.keyboard.press('Tab')
                page.screenshot(path=str(args.output/f'{width}-{initial}-focus.png'))
                page.keyboard.press('Enter');state('selected' if initial=='default' else 'default')
                page.keyboard.press('Space');state(initial)
                page.mouse.move(e['center']['x'],e['center']['y']);page.wait_for_timeout(250)
                page.screenshot(path=str(args.output/f'{width}-{initial}-hover.png'))
                page.mouse.down();page.screenshot(path=str(args.output/f'{width}-{initial}-pressed.png'));page.mouse.up()
                state('selected' if initial=='default' else 'default')
                assert row()['bounds']==bounds,'interaction changed geometry'
                page.wait_for_timeout(600)
                revision=page.evaluate('JSON.parse(window.zorkStory.snapshot()).revision')
                page.wait_for_timeout(300)
                assert page.evaluate('JSON.parse(window.zorkStory.snapshot()).revision')==revision,'settled row continuously redraws'
                report.append({'width':width,'initial':initial,'keyboard':['Tab','Enter','Space'],'pointer':'hover, press, single activation','bounds':bounds})
            page.evaluate('window.zorkStory.select_story("navigation-long")')
            state('long');row();page.screenshot(path=str(args.output/f'{width}-long.png'))
        assert not errors,errors
        (args.output/'report.json').write_text(json.dumps({'backend':args.backend,'offline':True,'checks':report,'errors':errors},indent=2))
        browser.close()
        print('PASS browser navigation: keyboard, pointer, stable geometry, compact/wide, offline')
if __name__=='__main__':main()

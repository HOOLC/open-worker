# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Exercise the shared disclosure on GPUI Web using real pointer/keyboard input."""
import argparse
import json
from pathlib import Path
from playwright.sync_api import sync_playwright


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--url', required=True)
    parser.add_argument('--browser', type=Path, help='Optional existing Chromium executable')
    parser.add_argument('--output', type=Path, default=Path('artifacts/tab-collapse/web'))
    parser.add_argument('--backend', choices=['auto', 'webgl'], default='auto')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with sync_playwright() as pw:
        options = {'executable_path': str(args.browser)} if args.browser else {}
        browser = pw.chromium.launch(headless=True, **options)
        context = browser.new_context(viewport={'width': 400, 'height': 400})
        page = context.new_page()
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(args.url + '?story=navigation-fold-open&backend=' + args.backend)
        page.wait_for_function('document.documentElement.dataset.ready === "true"')
        context.set_offline(True)
        page.wait_for_function('JSON.parse(window.zorkStory.snapshot()).elements.some(e => e.id === "fold-following")')
        sample_js = '''() => JSON.parse(window.zorkStory.snapshot()).elements.find(e => e.id === "fold-following").bounds.y'''
        full = page.evaluate(sample_js)
        header = page.evaluate('JSON.parse(window.zorkStory.snapshot()).elements.find(e => e.id === "fold-header").center')
        # Collect every displayed position; the click is sent through Playwright.
        page.evaluate('''() => { window.foldSamples = []; window.foldCollect = true;
            const tick = () => { const e = JSON.parse(window.zorkStory.snapshot()).elements.find(e => e.id === 'fold-following');
                if (e) window.foldSamples.push(e.bounds.y); if (window.foldCollect) requestAnimationFrame(tick); }; tick(); }''')
        page.mouse.click(header['x'], header['y'])
        page.wait_for_timeout(55)
        page.mouse.click(header['x'], header['y'])
        page.wait_for_timeout(450)
        assert abs(page.evaluate(sample_js) - full) < 0.1
        values = page.evaluate('window.foldCollect = false; window.foldSamples')
        assert any(full - 136 < value < full for value in values), values
        assert min(values) < full and values[-1] == full, values
        page.screenshot(path=str(args.output / 'reopened.png'))
        page.mouse.click(header['x'], header['y'])
        page.wait_for_timeout(80)
        page.screenshot(path=str(args.output / 'closing.png'))
        page.wait_for_timeout(370)
        assert abs(page.evaluate(sample_js) - full + 136) < 0.1
        assert not page.evaluate('JSON.parse(window.zorkStory.snapshot()).elements.some(e => e.id.startsWith("fold-task-"))')
        page.screenshot(path=str(args.output / 'closed.png'))
        page.mouse.move(399, 399)
        page.wait_for_timeout(600)
        revision = page.evaluate('JSON.parse(window.zorkStory.snapshot()).revision')
        page.wait_for_timeout(250)
        assert page.evaluate('JSON.parse(window.zorkStory.snapshot()).revision') == revision
        # Header remains keyboard-operable after the children have been removed.
        page.mouse.click(header['x'], header['y'])
        page.wait_for_timeout(450)
        page.keyboard.press('Space')
        page.wait_for_timeout(450)
        assert abs(page.evaluate(sample_js) - full + 136) < 0.1
        assert not errors, errors
        (args.output / 'report.json').write_text(json.dumps({'backend': args.backend, 'samples': values,
            'full_y': full, 'idle_revision': revision, 'offline': True, 'errors': errors}, indent=2))
        browser.close()
    print('PASS Web collapse: continuous reversal, height/gap, unmount, keyboard, idle, offline')


if __name__ == '__main__':
    main()

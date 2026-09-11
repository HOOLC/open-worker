# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Capture the original WGSL -> Metal compiler output for comparison."""
from pathlib import Path
import functools,http.server,threading,json
from playwright.sync_api import sync_playwright
root=Path(__file__).resolve().parents[1]
class Quiet(http.server.SimpleHTTPRequestHandler):
 def log_message(self,*args):pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Quiet,directory=str(root)))
threading.Thread(target=server.serve_forever,daemon=True).start()
try:
 with sync_playwright() as pw:
  chrome=sorted((Path.home()/'Library/Caches/ms-playwright').glob('chromium-*/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'))[-1]
  browser=pw.chromium.launch(headless=True,executable_path=str(chrome),args=['--enable-unsafe-webgpu','--enable-dawn-features=dump_shaders,disable_symbol_renaming'])
  try:
   logs=[];page=browser.new_page();page.on('console',lambda m:logs.append(dict(type=m.type,text=m.text)))
   page.goto(f'http://127.0.0.1:{server.server_port}/')
   page.evaluate('''async()=>{const {parse}=await import('/vendor/package/dist/index.js');await parse('let x = 42;');}''')
   (root/'experiments/dawn-shaders.json').write_text(json.dumps(logs,indent=2))
   print([(r['type'],len(r['text']),r['text'][:80]) for r in logs])
  finally:browser.close()
finally:server.shutdown();server.server_close()

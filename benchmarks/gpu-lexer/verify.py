# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Compare Rust tokenizer exactly and native GPU labels with upstream JS/WebGPU."""
import functools, http.server, json, os, subprocess, threading
from pathlib import Path
from playwright.sync_api import sync_playwright
root=Path(__file__).resolve().parent
binary=Path(os.environ['CARGO_TARGET_DIR'])/'release/zork-gpu-lexer-bench'
source=(root/'vendor/package/dist/index.js').read_text()+'\nexport {WA as tokenize, YA as engine};'
class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self,*args): pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Quiet,directory=str(root)))
threading.Thread(target=server.serve_forever,daemon=True).start()
try:
    with sync_playwright() as pw:
        chrome=Path(os.environ['CHROME_BIN']) if 'CHROME_BIN' in os.environ else sorted((Path.home()/'Library/Caches/ms-playwright').glob('chromium-*/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'))[-1]
        browser=pw.chromium.launch(headless=True,executable_path=str(chrome),args=['--enable-unsafe-webgpu'])
        page=browser.new_page();page.goto(f'http://127.0.0.1:{server.server_port}/')
        page.evaluate('''async source => {window.upstream = await import(URL.createObjectURL(new Blob([source],{type:'text/javascript'})));window.engine=upstream.engine();await engine.i();}''',source)
        results=[]
        manifest=json.loads((root/'fixtures/manifest.json').read_text())
        files=['utf8.txt']+[f['file'] for f in manifest]
        for file in files:
            path=root/'fixtures'/file; code=path.read_text()
            # read_text normalizes CRLF; use exact decoded bytes on both sides.
            code=path.read_bytes().decode()
            ref=page.evaluate('''async code=>{let t=upstream.tokenize(code);let labels=await engine.r(t[0],t[2],words=>Array.from({length:t[3]},(_,i)=>(words[i>>2]>>((i&3)*8))&255));return {features:Array.from(t[0]),ranges:Array.from(t[1]),labels}}''',code)
            rust=json.loads(subprocess.check_output([str(binary),'tokens',str(path)]))
            assert rust['features']==ref['features'],('features',file)
            assert rust['ranges']==ref['ranges'],('ranges',file)
            # Start one native process per fixture: numerical parity, not timing.
            labels=json.loads(subprocess.check_output([str(binary),'labels',str(path)]))
            assert len(labels)==len(ref['labels'])
            mismatches=sum(a!=b for a,b in zip(labels,ref['labels']))
            row=dict(file=file,tokens=len(labels),tokenizer_exact=True,label_mismatches=mismatches,label_agreement=1-mismatches/max(1,len(labels)))
            results.append(row);print(row,flush=True)
        (root/'results/parity.json').write_text(json.dumps(dict(browser=browser.version,results=results),indent=2)+'\n')
        browser.close()
finally: server.shutdown();server.server_close()

# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Exact-label checks against upstream including tiny/tree boundaries and reuse."""
from pathlib import Path
import functools,http.server,json,os,random,subprocess,threading,hashlib
from playwright.sync_api import sync_playwright
root=Path(__file__).resolve().parent
out=root/'results/optimization'
cases=[]
for manifest in ['manifest.json','sweep-manifest.json']:
    for row in json.loads((root/'fixtures'/manifest).read_text()):
        cases.append(dict(id=row['file'],code=(root/'fixtures'/row['file']).read_bytes().decode()))
cases.append(dict(id='empty',code=''))
for count in [1,2,3,4,5,7,8,15,16,17,31,32,33,63,64,65,127,128,129,255,256,257,511,512,513,1023,1024,1025]:
    for pattern in ['x;', '"/*#{}()0;', '猫;', '🐈;']:
        cases.append(dict(id=f'boundary-{count}-{pattern}',code=(pattern*(count//len(pattern)+1))[:count]))
rng=random.Random(20260909)
parts=['let','fn','if','42','"','\'','\\','/','*','//','\n','\r\n','\t',' ','你好','🐈','::','?','(',')','{','}','[',']','=>','-','+',';','αβ']
for index in range(120):
    cases.append(dict(id=f'mixed-{index}',code=''.join(rng.choice(parts) for _ in range(rng.randint(1,96)))))
# Shuffle and repeat to exercise size changes and residual workgroup/buffer memory.
rng.shuffle(cases)
cases=cases+list(reversed(cases[:80]))
(out/'verification-cases.json').write_text(json.dumps(cases,ensure_ascii=False)+'\n')
source=(root/'vendor/package/dist/index.js').read_text()+'\nexport {WA as tokenize,YA as engine};'
class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self,*args):pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Quiet,directory=str(root)))
threading.Thread(target=server.serve_forever,daemon=True).start()
try:
    with sync_playwright() as pw:
        chrome=Path(os.environ['CHROME_BIN']) if 'CHROME_BIN' in os.environ else sorted((Path.home()/'Library/Caches/ms-playwright').glob('chromium-*/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'))[-1]
        browser=pw.chromium.launch(headless=True,executable_path=str(chrome),args=['--enable-unsafe-webgpu'])
        try:
            page=browser.new_page();page.goto(f'http://127.0.0.1:{server.server_port}/')
            page.evaluate('''async source=>{window.ref=await import(URL.createObjectURL(new Blob([source],{type:'text/javascript'})));window.engine=ref.engine();await engine.i();}''',source)
            reference=[]
            for case in cases:
                labels=page.evaluate('''async code=>{const t=ref.tokenize(code);if(!t[3])return [];return await engine.r(t[0],t[2],words=>Array.from({length:t[3]},(_,i)=>(words[i>>2]>>((i&3)*8))&255));}''',case['code'])
                reference.append(labels)
        finally:browser.close()
finally:server.shutdown();server.server_close()
binary=Path(os.environ['CARGO_TARGET_DIR'])/'release/zork-gpu-lexer-bench'
run=subprocess.run([str(binary),'labels-batch',str(out/'verification-cases.json')],text=True,capture_output=True,check=True,timeout=120)
(out/'verification-native.log').write_text(run.stderr)
actual=[json.loads(line) for line in run.stdout.splitlines()]
assert len(actual)==len(cases)
rows=[]
for case,ref,row in zip(cases,reference,actual):
    assert row['id']==case['id'] and len(row['labels'])==len(ref)
    mismatches=sum(a!=b for a,b in zip(ref,row['labels']))
    rows.append(dict(id=case['id'],tokens=len(ref),mismatches=mismatches))
report=dict(cases=len(rows),tokens=sum(r['tokens'] for r in rows),mismatches=sum(r['mismatches'] for r in rows),flags={k:v for k,v in os.environ.items() if k.startswith('LEXER_')},binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),results=rows)
(out/'verification.json').write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
print({k:v for k,v in report.items() if k!='results'})
if report['mismatches']:
    print([r for r in rows if r['mismatches']][:20]);raise SystemExit(1)

# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Shiki 4.4.3, two regex engines, fixed byte sweep in dedicated browser workers."""
import functools,http.server,json,os,subprocess,threading,time,hashlib
from pathlib import Path
from playwright.sync_api import sync_playwright
root=Path(__file__).resolve().parent
results=root/'results/shiki';results.mkdir(exist_ok=True)
class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self,*args):pass
    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy','same-origin')
        self.send_header('Cross-Origin-Embedder-Policy','require-corp')
        super().end_headers()
def busy():
    return [p for p in subprocess.check_output(['ps','-axo','comm='],text=True).splitlines()
            if Path(p.strip()).name in {'cargo','rustc','clang','clang++','zork-gui-render-bench','zork-gpu-lexer-bench'}]
worker=r'''
let highlighter;
const languages={rs:'rust',ts:'typescript',py:'python',json:'json',yaml:'yaml'};
function highlight(code,lang){
    const result=highlighter.codeToTokens(code,{lang:languages[lang],theme:'github-light',tokenizeTimeLimit:0,tokenizeMaxLineLength:0});
    const spans=[];let end=0;
    function add(start,finish,color,fontStyle=0){
        if(finish===start)return;
        const last=spans.at(-1);
        if(last && last.end===start && last.color===color && last.fontStyle===fontStyle)last.end=finish;
        else spans.push({start,end:finish,color,fontStyle});
    }
    for(const line of result.tokens)for(const token of line){
        if(token.offset>end)add(end,token.offset,result.fg);
        add(token.offset,token.offset+token.content.length,token.color??result.fg,token.fontStyle??0);
        end=token.offset+token.content.length;
    }
    if(end<code.length)add(end,code.length,result.fg);
    return {spans,tokens:result.tokens};
}
function validate(code,result){
    let end=0;
    for(const s of result.spans){if(s.start!==end||s.end<s.start)throw Error('Invalid span');end=s.end;}
    if(end!==code.length)throw Error('Incomplete result');
    for(const line of result.tokens)for(const t of line){
        if(code.slice(t.offset,t.offset+t.content.length)!==t.content)throw Error('Token/source mismatch');
    }
}
self.onmessage=async({data})=>{
    try{
        if(data.init){
            let start=performance.now();const module=await import(data.url);const import_ms=performance.now()-start;
            start=performance.now();highlighter=await module.init();const init_ms=performance.now()-start;
            self.postMessage({version:module.version,import_ms,init_ms,languages:highlighter.getLoadedLanguages(),cross_origin_isolated:self.crossOriginIsolated});return;
        }
        if(data.dispose){highlighter.dispose();self.postMessage({disposed:true});return;}
        let start=performance.now();const first=highlight(data.code,data.language);const first_ms=performance.now()-start;
        validate(data.code,first);
        const times=[];let checksum=0;
        for(let i=0;i<data.repetitions;i++){
            start=performance.now();const result=highlight(data.code,data.language);times.push(performance.now()-start);
            checksum+=result.spans.length;validate(data.code,result);
        }
        times.sort((a,b)=>a-b);
        self.postMessage({first_ms,median_ms:times[Math.floor(times.length/2)],p95_ms:times[Math.ceil(times.length*.95)-1],min_ms:times[0],times_ms:times,spans:first.spans.length,checksum});
    }catch(error){self.postMessage({error:String(error.stack||error)});}
};
'''
manifest=json.loads((root/'fixtures/sweep-manifest.json').read_text())
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Quiet,directory=str(root)))
threading.Thread(target=server.serve_forever,daemon=True).start()
try:
    with sync_playwright() as pw:
        chrome=Path(os.environ['CHROME_BIN']) if 'CHROME_BIN' in os.environ else sorted((Path.home()/'Library/Caches/ms-playwright').glob('chromium-*/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'))[-1]
        for round,repetitions in [(1,100),(2,30)]:
            for engine in (['oniguruma','javascript'] if round==1 else ['javascript','oniguruma']):
                attempt=0
                while True:
                    attempt+=1
                    while busy():time.sleep(2)
                    browser=pw.chromium.launch(headless=True,executable_path=str(chrome))
                    try:
                        page=browser.new_page();page.goto(f'http://127.0.0.1:{server.server_port}/')
                        page.evaluate('''source=>{window.worker=new Worker(URL.createObjectURL(new Blob([source],{type:'text/javascript'})),{type:'module'});window.request=data=>new Promise((resolve,reject)=>{worker.onmessage=e=>e.data.error?reject(Error(e.data.error)):resolve(e.data);worker.onerror=e=>reject(Error(e.message));worker.postMessage(data);});}''',worker)
                        init=page.evaluate('engine=>request({init:true,url:new URL(`/shiki/dist/${engine}.js`,location.href).href})',engine)
                        print(engine,'initialized',init,flush=True)
                        overlaps=[];stop=threading.Event()
                        def monitor():
                            while not stop.is_set():
                                competitors=busy()
                                if competitors:overlaps.append(competitors)
                                stop.wait(.25)
                        watcher=threading.Thread(target=monitor);watcher.start();rows=[]
                        try:
                            for fixture in manifest:
                                if overlaps:break
                                code=(root/'fixtures'/fixture['file']).read_bytes().decode()
                                measured=page.evaluate('data=>request(data)',dict(code=code,language=fixture['language'],repetitions=repetitions))
                                rows.append(dict(**fixture,**measured));print(round,engine,fixture['file'],f"{measured['median_ms']:.4f} ms",flush=True)
                        finally:stop.set();watcher.join()
                        page.evaluate('()=>request({dispose:true})')
                        result=dict(engine=engine,browser=browser.version,init=init,repetitions=repetitions,overlaps=overlaps,results=rows,bundle_sha256=hashlib.sha256((root/f'shiki/dist/{engine}.js').read_bytes()).hexdigest(),date_utc=subprocess.check_output(['date','-u'],text=True).strip())
                        filename=f'{engine}-{round}.json' if not overlaps else f'{engine}-{round}-discarded-{attempt}.json'
                        (results/filename).write_text(json.dumps(result,indent=2)+'\n')
                        if not overlaps:break
                        print('Discarding overlapping run',flush=True)
                    finally:browser.close()
finally:server.shutdown();server.server_close()

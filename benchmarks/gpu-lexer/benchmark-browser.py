# /// script
# dependencies = ["playwright==1.58.0"]
# ///
"""Benchmark the unchanged public npm API in a dedicated Chrome WebGPU worker."""
import functools
import http.server
import json
import os
import subprocess
import threading
import time
from pathlib import Path
from playwright.sync_api import sync_playwright

root = Path(__file__).resolve().parent
results = root / 'results'

class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        super().end_headers()

def busy():
    processes = subprocess.check_output(['ps', '-axo', 'comm='], text=True)
    return [line for line in processes.splitlines() if Path(line.strip()).name in
            {'rustc', 'cargo', 'clang', 'clang++', 'zork-gui-render-bench', 'zork-gpu-lexer-bench'}]

worker_source = r'''
let parse;
self.onmessage = async ({data}) => {
  try {
    if (data.init) {
      const start = performance.now();
      ({parse} = await import(data.url));
      const importMs = performance.now() - start;
      const adapter = await navigator.gpu.requestAdapter();
      if (!adapter) throw new Error('No WebGPU adapter');
      self.postMessage({import_ms: importMs, adapter: {
        vendor: adapter.info.vendor, architecture: adapter.info.architecture,
        device: adapter.info.device, description: adapter.info.description,
        isFallbackAdapter: adapter.info.isFallbackAdapter,
        features: Array.from(adapter.features),
      }, user_agent: navigator.userAgent});
      return;
    }
    const start = performance.now();
    const first = await parse(data.code);
    const firstMs = performance.now() - start;
    function validate(spans) {
      let end = 0;
      for (const span of spans) {
        if (span.start !== end || span.end < span.start) throw new Error('Invalid span');
        end = span.end;
      }
      if (end !== data.code.length) throw new Error('Incomplete highlighting');
    }
    validate(first);
    const times = [];
    let spanCount = 0;
    for (let i = 0; i < data.repetitions; i++) {
      const start = performance.now();
      const spans = await parse(data.code);
      times.push(performance.now() - start);
      spanCount += spans.length;
      validate(spans);
    }
    times.sort((a,b) => a-b);
    self.postMessage({first_ms:firstMs, median_ms:times[Math.floor(times.length/2)],
      p95_ms:times[Math.ceil(times.length*.95)-1], min_ms:times[0],
      times_ms:times, first_spans:first.length, span_count_checksum:spanCount});
  } catch (error) {self.postMessage({error:String(error.stack || error)});}
};
'''
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Quiet, directory=str(root)))
threading.Thread(target=server.serve_forever, daemon=True).start()
manifest = json.loads((root / os.environ.get('LEXER_MANIFEST', 'fixtures/manifest.json')).read_text())
result_prefix = os.environ.get('LEXER_RESULT_PREFIX', 'browser')
try:
    with sync_playwright() as pw:
        chrome = Path(os.environ['CHROME_BIN']) if 'CHROME_BIN' in os.environ else sorted(
            (Path.home() / 'Library/Caches/ms-playwright').glob(
                'chromium-*/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'))[-1]
        for round, repetitions in [(1, int(os.environ.get('LEXER_REPS', '30'))), (2, int(os.environ.get('LEXER_CONFIRM_REPS', '10')))]:
            attempt = 0
            while True:
                attempt += 1
                while busy():
                    time.sleep(2)
                browser = pw.chromium.launch(headless=True, executable_path=str(chrome), args=['--enable-unsafe-webgpu'])
                try:
                    system = browser.new_browser_cdp_session().send('SystemInfo.getInfo')
                    page = browser.new_page()
                    page.goto(f'http://127.0.0.1:{server.server_port}/')
                    page.evaluate('''source => {
                      window.worker = new Worker(URL.createObjectURL(new Blob([source], {type:'text/javascript'})), {type:'module'});
                      window.request = data => new Promise((resolve,reject) => {
                        worker.onmessage = e => e.data.error ? reject(new Error(e.data.error)) : resolve(e.data);
                        worker.onerror = e => reject(new Error(e.message));
                        worker.postMessage(data);
                      });
                    }''', worker_source)
                    info = page.evaluate('''() => request({init:true,url:new URL('/vendor/package/dist/index.js',location.href).href})''')
                    print('Adapter:', info['adapter'], flush=True)
                    if info['adapter'].get('isFallbackAdapter'):
                        raise RuntimeError('Software fallback is not a comparable GPU benchmark')
                    overlaps = []
                    stop = threading.Event()
                    def monitor():
                        while not stop.is_set():
                            competitors = busy()
                            if competitors:
                                overlaps.append(competitors)
                            stop.wait(.25)
                    watcher = threading.Thread(target=monitor)
                    watcher.start()
                    rows = []
                    try:
                        for fixture in manifest:
                            if overlaps:
                                break
                            code = (root / 'fixtures' / fixture['file']).read_bytes().decode()
                            measured = page.evaluate('data => request(data)', dict(code=code,repetitions=repetitions))
                            rows.append(dict(**fixture, bytes=len(code.encode()), **measured))
                            print(f"round {round}: {fixture['file']} {measured['median_ms']:.3f}ms", flush=True)
                    finally:
                        stop.set()
                        watcher.join()
                    result = dict(browser=browser.version, system_info=system, worker_info=info,
                                  repetitions=repetitions, overlaps=overlaps, results=rows,
                                  date_utc=subprocess.check_output(['date','-u'],text=True).strip())
                    name = f'{result_prefix}-{round}.json' if not overlaps else f'{result_prefix}-{round}-discarded-{attempt}.json'
                    (results / name).write_text(json.dumps(result,indent=2)+'\n')
                    if not overlaps:
                        break
                    print('Compiler/test overlap: discarding and retrying this round',flush=True)
                finally:
                    browser.close()
finally:
    server.shutdown()
    server.server_close()

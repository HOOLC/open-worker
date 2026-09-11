#!/usr/bin/env python3
"""Exercise the packaged CEF runtime without any installed browser or live account."""
import argparse
import json
import os
import queue
import sqlite3
import struct
import sys
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

class Runtime:
    def __init__(self, executable, profile):
        self.log = (profile / 'probe-process.log').open('ab')
        # Match production: the desktop owns the real browser process and its
        # inherited pipes. Run this controller in a GUI session on macOS.
        arguments = [str(executable), str(profile)]
        if os.environ.get('ZORK_RUNTIME_TRACE'):
            arguments.append(f'--log-net-log={profile / "network.json"}')
        env = dict(os.environ)
        if sys.platform == 'darwin':
            env['MallocNanoZone'] = '0'
        self.p = subprocess.Popen(arguments, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=self.log, env=env)
        self.messages = queue.Queue()
        self.events = []
        self.next = 0
        threading.Thread(target=self.read, daemon=True).start()
        try:
            ready = self.messages.get(timeout=20)
            assert ready.get('method') == 'Zork.ready', ready
        except BaseException:
            self.dispose()
            raise

    def read(self):
        def exact(size):
            chunks = bytearray()
            while len(chunks) < size:
                data = self.p.stdout.read(size - len(chunks))
                if not data:
                    raise EOFError('browser runtime disconnected')
                chunks.extend(data)
            return bytes(chunks)
        try:
            while True:
                head, count = struct.unpack('<II', exact(8))
                assert head <= 16 * 1024 * 1024 and count <= 128 * 1024 * 1024
                message = json.loads(exact(head))
                pixels = exact(count)
                if pixels:
                    message['pixel_bytes'] = len(pixels)
                self.messages.put(message)
        except Exception as error:
            self.messages.put({'transport_error': str(error)})

    def call(self, method, params=None, session=None):
        self.next += 1
        if os.environ.get("ZORK_RUNTIME_TRACE"): print("CALL", method, self.next, session, flush=True)
        request = {'id': self.next, 'method': method, 'params': params or {}, 'sessionId': session}
        self.p.stdin.write((json.dumps(request) + '\n').encode())
        self.p.stdin.flush()
        deadline = time.monotonic() + (600 if os.environ.get('ZORK_RUNTIME_PERMISSION_WAIT') else 20)
        while True:
            message = self.messages.get(timeout=max(.001, deadline - time.monotonic()))
            if os.environ.get("ZORK_RUNTIME_TRACE"): print("RECEIVED", str(message)[:700], flush=True)
            assert 'transport_error' not in message, message
            if message.get('id') == self.next:
                assert 'error' not in message, message
                return message['result']
            self.events.append(message)

    def evaluate(self, tab, expression):
        result = self.call('Runtime.evaluate', {'expression': expression, 'returnByValue': True, 'awaitPromise': True}, tab)
        assert 'exceptionDetails' not in result, result
        return result['result'].get('value')

    def wait_value(self, tab, expression, expected, timeout=15):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if self.evaluate(tab, expression) == expected:
                return
            time.sleep(.05)
        raise AssertionError((expression, expected))

    def open(self, url):
        tab = self.call('Target.createTarget', {'url': 'about:blank'})['targetId']
        self.call('Page.enable', session=tab)
        self.call('Page.navigate', {'url': url}, tab)
        self.wait_value(tab, 'document.readyState', 'complete')
        return tab

    def close(self):
        try:
            if self.p.poll() is None:
                self.call('Browser.close')
                self.p.wait(timeout=15)
        finally:
            self.dispose()

    def dispose(self):
        if self.p.poll() is None:
            self.p.kill()
        self.p.wait()
        self.p.stdin.close()
        self.p.stdout.close()
        self.log.close()



def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--startup-only', action='store_true',
                        help='Check packaged bundle startup, renderer and paint without opening a website')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    profile = Path(tempfile.mkdtemp(prefix='zork-cef-validation-'))
    class Page(BaseHTTPRequestHandler):
        def log_message(self, *_): pass
        def do_GET(self):
            if os.environ.get("ZORK_RUNTIME_TRACE"): print("HTTP",self.path,flush=True)
            if self.path.startswith('/download'):
                self.send_response(200)
                self.send_header('Content-Disposition', 'attachment; filename=browser-fixture.txt')
                self.send_header('Content-Type', 'application/octet-stream')
                body = b'Zork embedded browser download\n'
            else:
                self.send_response(200)
                body = ('''<!doctype html><meta charset=utf-8><title>CEF fixture</title>
                <input id=input><button id=button onclick="this.textContent='clicked'">click</button>
                <a id=download href=/download>download</a><p id=cookie>''' + ('session-ok' if 'persistent=fixture' in self.headers.get('Cookie', '') else 'fresh') + '</p>').encode()
                self.send_header('Set-Cookie', 'persistent=fixture; Max-Age=3600; Path=/; HttpOnly; SameSite=Lax')
                self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Page)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f'http://127.0.0.1:{server.server_port}/'
    runtime = None
    report = {'engine': 'CEF', 'runtime': str(args.runtime.resolve()), 'profile': str(profile)}
    try:
        runtime = Runtime(args.runtime.resolve(), profile)
        if args.startup_only:
            tab = runtime.open('about:blank')
            runtime.call('Zork.viewport', {'width':600,'height':500,'visible':True,'scale':1}, tab)
            runtime.wait_value(tab, 'document.visibilityState', 'visible')
            runtime.evaluate(tab, 'new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))))')
            assert runtime.evaluate(tab, '6 * 7') == 42
            runtime.evaluate(tab, 'document.body.style.background = "rgb(20,40,60)"')
            paint_deadline = time.monotonic() + 5
            while not any(event.get('pixel_bytes') for event in runtime.events) and time.monotonic() < paint_deadline:
                runtime.evaluate(tab, 'document.readyState')
                time.sleep(.05)
            assert any(event.get('pixel_bytes') for event in runtime.events), 'renderer did not paint'
            # No website or cookies are used in this startup check. Disconnect
            # the controller and verify the owned process exits on its own.
            runtime.p.stdin.close()
            runtime.p.wait(timeout=15)
            runtime.close()
            runtime = None
            report.update(status='passed', startup_only=True, renderer=True, native_paint=True, parent_pipe_eof_shutdown=True)
            print(json.dumps(report, ensure_ascii=False, indent=2))
            return
        tab = runtime.open(url)
        first_tab = tab
        runtime.wait_value(tab, 'document.title', 'CEF fixture')
        runtime.wait_value(tab, 'document.visibilityState', 'hidden')
        runtime.call('Zork.viewport', {'width':600,'height':500,'visible':True,'scale':2}, tab)
        runtime.wait_value(tab, 'document.visibilityState', 'visible')
        runtime.wait_value(tab, 'innerWidth', 600)
        runtime.wait_value(tab, 'devicePixelRatio', 2)
        runtime.evaluate(tab, 'new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))))')
        runtime.evaluate(tab, 'document.querySelector("#input").focus()')
        runtime.call('Input.insertText', {'text':'中文输入'}, tab)
        runtime.wait_value(tab, 'document.querySelector("#input").value', '中文输入')
        point = runtime.evaluate(tab, '(()=>{const r=document.querySelector("#button").getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()')
        for kind in ['mousePressed','mouseReleased']:
            runtime.call('Input.dispatchMouseEvent',dict(point,type=kind,button='left',clickCount=1),tab)
        runtime.wait_value(tab, 'document.querySelector("#button").textContent', 'clicked')
        print('Checking CEF screenshot', flush=True)
        shot = runtime.call('Page.captureScreenshot', {'format':'jpeg','captureBeyondViewport':False}, tab)
        assert len(shot.get('data', '')) > 100
        report['screenshot'] = True
        runtime.evaluate(tab, 'document.querySelector("#download").click()')
        end = time.monotonic() + 15
        while time.monotonic() < end:
            if list((profile/'downloads').glob('browser-fixture.txt')): break
            time.sleep(.1)
        download = profile/'downloads/browser-fixture.txt'
        assert download.read_bytes() == b'Zork embedded browser download\n'
        report.update(native_input=True, download=True, dpr=2, hidden_tab_visibility=True,
                      user_agent=runtime.evaluate(tab,'navigator.userAgent'),
                      webdriver=runtime.evaluate(tab,'navigator.webdriver'))
        report['cookies_before_close'] = len(runtime.call('Network.getCookies', {'urls':[url]}, tab)['cookies'])
        runtime.close()
        report['paint_frames'] = len([e for e in runtime.events if e.get('pixel_bytes')])
        assert report['paint_frames'] > 0
        runtime = Runtime(args.runtime.resolve(), profile)
        tab = runtime.open(url)
        runtime.wait_value(tab, 'document.querySelector("#cookie")?.textContent', 'session-ok')
        assert tab != first_tab, 'page identities cannot be reused after a process restart'
        report['page_ids_unique_across_restart'] = True
        report['http_only_cookie_survives_restart'] = True
        runtime.close()
        databases = list(profile.rglob('Cookies'))
        assert databases
        with sqlite3.connect(databases[0]) as db:
            row = db.execute("select length(value),length(encrypted_value) from cookies where name='persistent'").fetchone()
        assert row and row[0] == 0 and row[1] > 0, ('cookie storage must be encrypted', row)
        report['cookies_encrypted_at_rest'] = True
        report['status'] = 'passed'
        print(json.dumps(report, ensure_ascii=False, indent=2))
    finally:
        try:
            if runtime: runtime.close()
        finally:
            server.shutdown()
            server.server_close()
            (args.output/'runtime.json').write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')

if __name__ == '__main__': main()

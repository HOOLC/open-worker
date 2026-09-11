#!/usr/bin/env python3
"""Real Gateway/Agent, embedded Synch access client, HTTP and WebSocket service.

Build the affected binaries first. Uses isolated identities and a fake model;
never touches an installed node, user workspace or personal mobile device.
"""
import base64
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import select
import shlex
import signal
import socket
import subprocess
import sys
import tempfile
import threading
from urllib.parse import urlsplit
from urllib.request import Request, urlopen
from urllib.error import HTTPError

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/lib"))
from build_env import build_environment
spec = importlib.util.spec_from_file_location("mesh_fixture", ROOT / "scripts/test-mesh.py")
f = importlib.util.module_from_spec(spec)
spec.loader.exec_module(f)
env = build_environment()
f.TARGET = Path(os.environ.get("ZORK_TEST_BIN_DIR", str(Path(env.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug")))
LARGE = b"mesh service payload\n" * 100000


class Page(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        if self.path == "/socket":
            accept = base64.b64encode(hashlib.sha1((self.headers["Sec-WebSocket-Key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
            self.send_response(101)
            self.send_header("Upgrade", "websocket")
            self.send_header("Connection", "Upgrade")
            self.send_header("Sec-WebSocket-Accept", accept)
            self.end_headers()
            try:
                while True:
                    prefix = self.rfile.read(2)
                    if len(prefix) < 2:
                        break
                    size = prefix[1] & 127
                    if size > 125:
                        break
                    mask = self.rfile.read(4)
                    data = self.rfile.read(size)
                    data = bytes(byte ^ mask[i % 4] for i, byte in enumerate(data))
                    if prefix[0] & 15 == 8:
                        break
                    self.wfile.write(bytes([0x81, len(data)]) + data)
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass
            self.close_connection = True
            return
        body = LARGE if self.path == "/large" else (b'{"mesh":"ok"}' if self.path == "/api" else b'''<!doctype html><meta charset=utf-8><title>Mesh service</title>
<h1>Shared through Mesh</h1><div id=api>loading</div><div id=ws>loading</div>
<script>fetch('/api').then(r=>r.json()).then(v=>document.querySelector('#api').textContent=v.mesh);
let s=new WebSocket('ws://'+location.host+'/socket');s.onopen=()=>s.send('websocket-ok');
s.onmessage=e=>document.querySelector('#ws').textContent=e.data;</script>''')
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Content-Type", "application/json" if self.path == "/api" else "text/html")
        self.send_header("Set-Cookie", "preview=mesh; Path=/; SameSite=Strict")
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        result = hashlib.sha256(body).hexdigest().encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(result)))
        self.end_headers()
        self.wfile.write(result)

    def log_message(self, *_):
        pass


def request(local, path, method="GET", body=None, extra=None):
    url = urlsplit(local)
    client = http.client.HTTPConnection("127.0.0.1", url.port, timeout=15)
    client.request(method, path, body, {"Host": url.netloc, **(extra or {})})
    response = client.getresponse()
    result = response.status, response.read()
    client.close()
    return result


def websocket(local):
    url = urlsplit(local)
    stream = socket.create_connection(("127.0.0.1", url.port), timeout=10)
    stream.sendall((f"GET /socket HTTP/1.1\r\nHost: {url.netloc}\r\nOrigin: http://{url.netloc}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").encode())
    head = b""
    while not head.endswith(b"\r\n\r\n"):
        byte = stream.recv(1)
        assert byte, head
        head += byte
    assert b"101 Switching Protocols" in head, head
    mask, data = b"abcd", b"mesh websocket"
    stream.sendall(bytes([0x81, 0x80 | len(data)]) + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))
    assert stream.recv(2) == bytes([0x81, len(data)])
    assert stream.recv(len(data)) == data
    return stream


def main():
    root = Path(tempfile.mkdtemp(prefix="zork-shared-services-"))
    node = client = process = page = None
    checks = []
    children = set()
    marker = str(root / "fixture-child")
    try:
        node, client = f.Node(root / "node"), f.Node(root / "client")
        node.config["admin"] = {"token": "isolated-service-fixture"}
        def authorized_request(method, path, body=None):
            req = Request(node.url + path, data=None if body is None else json.dumps(body).encode(), method=method,
                          headers={"Content-Type": "application/json", "Authorization": "Bearer isolated-service-fixture"})
            try: response = urlopen(req, timeout=25)
            except HTTPError as error: response = error
            with response:
                raw = response.read()
                return response.status, (json.loads(raw) if response.headers.get_content_type() == "application/json" else raw.decode()) if raw else None
        node.request = authorized_request
        node.config["mesh"]["peers"] = [{"origin": client.origin, "name": "client", "addr": f"127.0.0.1:{client.udp}", "execute": [], "client": True}]
        (node.root / "config.json").write_text(json.dumps(node.config))
        client.config["mesh"]["peers"] = [{"origin": node.origin, "name": "node", "addr": f"127.0.0.1:{node.udp}", "execute": []}]
        client.config["mesh"]["workspaces"] = []
        (client.root / "config.json").write_text(json.dumps(client.config))
        node.start()
        f.wait(lambda: node.request("GET", "/readyz")[0] == 200, "node ready")
        status, leader = node.request("POST", "/v1/node/agents", {"id":"preview-leader","name":"Preview","role":"leader","profile_id":"fixture","model":"fixture-model","thinking":"off"})
        assert status in (200,201), (status,leader)
        status, opened = node.request("POST", "/v1/node/agents/preview-leader/open", {})
        assert status == 200, (status,opened)
        session = opened["session_id"]
        request_ids={}
        def api(action, **fields):
            if fields.get("request_id") in request_ids: fields["request_id"]=request_ids[fields["request_id"]]
            return node.request("POST", "/v1/services", {"session_id":session,"action":action,**fields})
        def success(action, **fields):
            status, body = api(action, **fields)
            assert status == 200, (action,status,body)
            return body
        def named(name):
            status, body = api("list")
            return next((item for item in body.get("services",[]) if item["name"]==name),None) if status == 200 else None
        def inspect(id): return success("inspect", id=id)
        def fake_tool(name, args, key):
            args=dict(args);alias=args.pop("request_id",None)
            before=set(request_ids.values())
            content=json.dumps({"fake_tools":[{"name":name,"input":args}]})
            status, body=node.request("POST",f"/v1/im/sessions/{session}/messages",{"content":content,"request_id":key})
            assert status in (200,202), (status,body)
            def completed():
                for segment in (node.root/'sessions'/session/'segments').glob('*.jsonl'):
                    for line in segment.read_text().splitlines():
                        event=json.loads(line).get('event',{})
                        result=event.get('result',{})
                        if event.get('kind')=='tool_result' and result.get('tool')==name and result['invocation_id'] not in before:
                            assert result['outcome']=='succeeded',result
                            return result['invocation_id']
            invocation=f.wait(completed,'Agent '+name)
            if alias:request_ids[alias]=invocation
        page=ThreadingHTTPServer(("127.0.0.1",0),Page)
        threading.Thread(target=page.serve_forever,daemon=True).start()
        fake_tool("service.attach", {"name":"preview","port":page.server_port,"request_id":"attach-preview"}, "attach-message")
        preview=f.wait(lambda:named("preview"),"Agent attaches external service")
        assert preview["mode"]=="external" and not preview["shared"] and preview["logs"] is None
        fake_tool("service.share", {"id":preview["id"],"request_id":"share-preview"}, "share-message")
        shared=f.wait(lambda:(v if (v:=named("preview")) and v["shared"] else None),"Agent shares service")
        checks.append("named_agent_tools_attach_and_share")
        replay=success("share",id=shared["id"],request_id="share-preview")
        assert replay["replayed"] and replay["url"]==shared["url"]
        assert api("attach",name="reserved",port=int(node.config["bind"]["runtime"].rsplit(":",1)[1]),request_id="reserved")[0]==400
        assert api("restart",id=shared["id"],request_id="external-restart")[0]==400
        other=node.new_task()["session_id"]
        assert node.request("POST","/v1/services",{"session_id":other,"action":"inspect","id":shared["id"]})[0]==400
        checks.append("owner_external_process_and_control_port_guards")
        log=(root/"listener.log").open("w")
        process=subprocess.Popen([str(f.TARGET/"examples/service-listener"),str(client.root),shared["url"]],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,text=True)
        assert select.select([process.stdout],[],[],40)[0]
        line=process.stdout.readline();assert line,(root/"listener.log").read_text()
        local=json.loads(line)["url"]
        assert request(local,"/api")== (200,b'{"mesh":"ok"}')
        assert request(local,"/large")== (200,LARGE)
        assert request(local,"/upload","POST",LARGE)[1]==hashlib.sha256(LARGE).hexdigest().encode()
        assert request(local,"/api",extra={"Origin":"https://untrusted.example"})[0]==502
        checks.append("http_large_response_upload_and_origin_guard")
        def open_link(url):
            process.stdin.write(json.dumps({"open":url})+"\n");process.stdin.flush()
            assert select.select([process.stdout],[],[],40)[0]
            return json.loads(process.stdout.readline())["url"]
        if os.environ.get("ZORK_SERVICE_CHROMIUM"):
            second=success("attach",name="second",port=page.server_port,request_id="second")
            second=success("share",id=second["id"],request_id="second-share")
            second_url=open_link(second["url"])
            subprocess.run(["node",str(ROOT/"scripts/test-shared-service-browser.mjs"),local,second_url,str(root/"browser.png")],env=env,check=True,timeout=60)
            checks.append("chromium_page_websocket_cookie_storage_isolation")
        stream=websocket(local)
        success("unshare",id=shared["id"],request_id="unshare-preview")
        assert stream.recv(1)==b"";stream.close()
        assert request(local,"/api")[0]==502
        assert success("share",id=shared["id"],request_id="share-preview")["replayed"]
        assert not inspect(shared["id"])["shared"]
        assert success("share",id=shared["id"],request_id="reshare-preview")["url"]==shared["url"]
        assert request(local,"/api")[0]==200
        checks.append("unshare_and_reshare_keep_identity_and_replay_cannot_undo")
        stream=websocket(local)
        node.config["mesh"]["peers"][0]["client"]=False
        (node.root/"config.json").write_text(json.dumps(node.config))
        assert stream.recv(1)==b"";stream.close()
        assert request(local,"/api")[0]==502
        node.config["mesh"]["peers"][0]["client"]=True
        (node.root/"config.json").write_text(json.dumps(node.config))
        checks.append("revocation_closes_existing_and_new_streams")

        managed_port=f.port()
        managed_script=node.workspace/"managed-preview.py"
        child_file=node.workspace/"children.txt"
        managed_script.write_text("""from http.server import BaseHTTPRequestHandler,HTTPServer
import os,sys,subprocess
child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(3600)',sys.argv[2]])
with open(sys.argv[3],'a') as output: output.write(str(child.pid)+'\\n')
print('stdout-started',flush=True)
print('stderr-started',file=sys.stderr,flush=True)
class Handler(BaseHTTPRequestHandler):
 def do_GET(self):
  body=str(os.getpid()).encode(); self.send_response(200); self.send_header('Content-Length',str(len(body))); self.end_headers(); self.wfile.write(body)
 def log_message(self,*args): pass
HTTPServer(('127.0.0.1',int(sys.argv[1])),Handler).serve_forever()
""")
        command=[sys.executable,"-u",str(managed_script),str(managed_port),marker,str(child_file)]
        start_args={"name":"managed","port":managed_port,"command":command,"cwd":str(node.workspace),"request_id":"start-managed"}
        fake_tool("service.start",start_args,"managed-message")
        managed=f.wait(lambda:named("managed"),"Agent registers managed service")
        def running():
            value=inspect(managed["id"])
            return value if value["ready"] and value.get("pid") else None
        live=f.wait(running,"managed service ready")
        first_pid=live["pid"]
        assert live["logs"]["node"]==node.origin
        def logs_ready():
            return b"stdout-started" in Path(live["logs"]["stdout"]).read_bytes() and b"stderr-started" in Path(live["logs"]["stderr"]).read_bytes()
        f.wait(logs_ready,"stdout/stderr files flushed")
        assert success("start",**start_args)["replayed"]
        assert inspect(managed["id"])["pid"]==first_pid
        managed_url=success("share",id=managed["id"],request_id="share-managed")["url"]
        managed_local=open_link(managed_url)
        assert int(request(managed_local,"/")[1])==first_pid
        checks.append("managed_start_identity_readiness_and_filesystem_logs")
        success("restart",id=managed["id"],request_id="restart-managed")
        restarted=f.wait(lambda:(v if (v:=running()) and v["pid"]!=first_pid else None),"managed restart")
        assert success("restart",id=managed["id"],request_id="restart-managed")["replayed"]
        assert inspect(managed["id"])["pid"]==restarted["pid"]
        success("stop",id=managed["id"],request_id="stop-managed")
        assert not inspect(managed["id"])["desired_running"] and not inspect(managed["id"])["ready"]
        assert success("restart",id=managed["id"],request_id="restart-managed")["replayed"]
        assert not inspect(managed["id"])["desired_running"]
        checks.append("restart_and_stop_retries_do_not_repeat_or_resurrect")
        def registry_ready():
            status,body=api("list")
            return body if status==200 else None
        node.stop();node.start()
        f.wait(registry_ready,"registry after graceful restart")
        assert not inspect(managed["id"])["desired_running"] and not inspect(managed["id"])["ready"]
        assert inspect(managed["id"])["url"]==managed_url
        assert request(local,"/api")[0]==200
        checks.append("stopped_intent_and_shared_links_survive_restart")
        success("restart",id=managed["id"],request_id="resume-managed")
        before_crash=f.wait(running,"managed resumed")
        node.restart_gateway()
        f.wait(registry_ready,"registry after Gateway crash")
        restored=f.wait(lambda:(v if (v:=running()) and v["pid"]!=before_crash["pid"] else None),"service restored after Gateway crash")
        assert restored["url"]==managed_url
        assert int(request(managed_local,"/")[1])==restored["pid"]
        checks.append("crash_guard_and_automatic_service_restore")
        service_port=page.server_port;page.shutdown();page.server_close();page=None
        assert request(local,"/api")[0]==502
        page=ThreadingHTTPServer(("127.0.0.1",service_port),Page)
        threading.Thread(target=page.serve_forever,daemon=True).start()
        assert request(local,"/api")[0]==200
        checks.append("external_backend_offline_recovery_on_same_link")
        success("unshare",id=managed["id"],request_id="unshare-managed")
        success("stop",id=managed["id"],request_id="stop-managed-final")
        node.stop();node.start();f.wait(registry_ready,"final restore")
        final=inspect(managed["id"])
        assert not final["shared"] and not final["desired_running"] and final["logs"]==live["logs"]
        checks.append("unshare_stop_and_log_paths_persist")
        broken=success("start",name="broken",port=f.port(),command=[str(root/"missing-program")],cwd=str(node.workspace),request_id="broken")
        failed=f.wait(lambda:(v if (v:=inspect(broken["id"]))["state"] in ("failed","exited") else None),"startup error captured")
        assert failed["last_exit_code"]==127 and "service_spawn_failed" in failed["last_error"]
        checks.append("inspect_reports_startup_failure_without_parsing_logs")
        process.stdin.close();assert process.wait(timeout=15)==0;process=None;log.close()
        children.update(int(pid) for pid in child_file.read_text().splitlines())
        def no_children():
            return all(marker not in subprocess.run(["ps","-p",str(pid),"-o","command="],capture_output=True,text=True).stdout for pid in children)
        f.wait(no_children,"service process groups reaped")
        checks.append("process_groups_and_client_listeners_cleaned_up")
        print(json.dumps({"checks":checks,"evidence":str(root)},indent=2))
        (root/"result.json").write_text(json.dumps({"checks":checks},indent=2))
    finally:
        if process:
            process.stdin.close()
            try: process.wait(timeout=15)
            except subprocess.TimeoutExpired: process.kill();process.wait()
        if node: node.stop()
        if client: client.stop()
        if page: page.shutdown();page.server_close()
        child_file=root/"node/workspace/children.txt"
        if child_file.exists(): children.update(int(pid) for pid in child_file.read_text().splitlines())
        for pid in children:
            command=subprocess.run(["ps","-p",str(pid),"-o","command="],capture_output=True,text=True).stdout
            if marker in command:
                try: os.kill(pid,signal.SIGTERM)
                except ProcessLookupError: pass
        print(root)

if __name__ == "__main__": main()

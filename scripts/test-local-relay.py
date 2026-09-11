#!/usr/bin/env python3
"""Real iroh relay with a loopback, in-memory pkarr HTTP store for local tests."""
import http.server,importlib.util,json,os,re,subprocess,tempfile,threading
from pathlib import Path
from urllib.request import urlopen
spec=importlib.util.spec_from_file_location('fixture',Path(__file__).with_name('test-mesh.py'));f=importlib.util.module_from_spec(spec);spec.loader.exec_module(f)
root=Path(tempfile.mkdtemp(prefix='zrelay-',dir='/tmp'));records={};counts={'put':0,'get':0}
class Discovery(http.server.BaseHTTPRequestHandler):
    def do_PUT(self):
        records[self.path]=self.rfile.read(int(self.headers['Content-Length']));counts['put']+=1;self.send_response(200);self.end_headers()
    def do_GET(self):
        counts['get']+=1;data=records.get(self.path);self.send_response(200 if data else 404);self.send_header('Content-Type','application/octet-stream');self.end_headers();self.wfile.write(data or b'')
    def log_message(self,*args):pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Discovery);threading.Thread(target=server.serve_forever,daemon=True).start()
relay_port,metrics_port=f.port(),f.port();config=root/'relay.toml';config.write_text(f'enable_relay = true\nhttp_bind_addr = "127.0.0.1:{relay_port}"\nenable_metrics = true\nmetrics_bind_addr = "127.0.0.1:{metrics_port}"\n')
log=(root/'relay.log').open('wb');relay=subprocess.Popen([str(f.ROOT/'target/local-relay/bin/iroh-relay'),'--dev','--config-path',str(config)],stdout=log,stderr=log)
try:
    f.wait(lambda:urlopen(f'http://127.0.0.1:{metrics_port}/metrics',timeout=2).status==200,'relay ready')
    env=dict(os.environ,ZORK_TEST_RELAY=f'http://127.0.0.1:{relay_port}',ZORK_TEST_DISCOVERY=f'http://127.0.0.1:{server.server_port}',ZORK_MESH_LOCAL_DISCOVERY='0')
    subprocess.run(['python3',str(Path(__file__).with_name('test-mesh-enrollment.py'))],env=env,check=True)
    subprocess.run(['python3',str(Path(__file__).with_name('test-client-mesh.py'))],env=env,check=True)
    metrics=urlopen(f'http://127.0.0.1:{metrics_port}/metrics').read().decode();(root/'metrics.txt').write_text(metrics)
    traffic={k:float(v) for k,v in re.findall(r'^(\S*(?:bytes|packets)\S*) (\d+(?:\.\d+)?)$',metrics,re.M)}
    assert any(v>0 for v in traffic.values()),traffic
    assert counts['put']>=2 and counts['get']>=1,counts
    result={'root':str(root),'discovery_requests':counts,'relay_traffic':traffic};out=f.ROOT/'artifacts/leader-worker/local-relay.json';out.write_text(json.dumps(result,indent=2));print('PASS: configured local relay + local discovery, no peer IP hints, remote client round-trip and 400 KB artifact');print(result)
finally:relay.terminate();relay.wait(timeout=8);log.close();server.shutdown();print(root)

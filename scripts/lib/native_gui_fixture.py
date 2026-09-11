"""Shared native automation client for current desktop regression fixtures."""
import importlib.util
import json
import subprocess
from pathlib import Path
from urllib.request import Request, urlopen

spec = importlib.util.spec_from_file_location('mesh_fixture', Path(__file__).resolve().parents[1] / 'test-mesh.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)
Node, wait, port = fixture.Node, fixture.wait, fixture.port

class Native:
    def __init__(self,node,root):
        self.node=node;self.root=root;self.url=f'http://127.0.0.1:{port()}'
        self.log=(root/'gui.log').open('ab');self.process=None
    def stop(self):
        if self.process:
            self.process.terminate()
            try:self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:self.process.kill();self.process.wait()
            self.process=None
    def ui(self,path,body=None):
        request=Request(self.url+path,data=None if body is None else json.dumps(body).encode(),headers={'Authorization':'Bearer mesh-native-fixture','Content-Type':'application/json'})
        with urlopen(request,timeout=8) as response:return response.read()
    def element(self,id,enabled=False):
        return next((e for e in json.loads(self.ui('/v1/elements'))['elements'] if e['id']==id and e['visible'] and (not enabled or e['enabled'])),None)
    def click(self,id):self.ui('/v1/actions',{'type':'click','target':{'element_id':id}})
    def type(self,text):self.ui('/v1/actions',{'type':'type_text','text':text})
    def screenshot(self,path):
        revision=json.loads(self.ui('/v1/elements'))['revision']
        self.ui(f'/v1/elements?after_revision={revision}&timeout_ms=1000')
        path.write_bytes(self.ui('/v1/screenshot'))

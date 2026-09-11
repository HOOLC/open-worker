#!/usr/bin/env python3
"""Local OIDC/PKCE fixture; never contacts a Cue production account."""
import base64,hashlib,json,os,subprocess,tempfile,threading,time
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from urllib.parse import parse_qs,urlparse,urlencode
from urllib.request import urlopen

def b64(b):return base64.urlsafe_b64encode(b).decode().rstrip('=')
root=Path(tempfile.mkdtemp(prefix='zork-oidc-'));key=root/'key.pem'
subprocess.run(['openssl','genrsa','-out',str(key),'2048'],check=True,capture_output=True)
modulus=subprocess.check_output(['openssl','rsa','-in',str(key),'-noout','-modulus'],stderr=subprocess.DEVNULL).decode().strip().split('=')[1]
n=b64(bytes.fromhex(modulus));auth={};checks=[]
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def reply(self,value,status=200):
        data=json.dumps(value).encode();self.send_response(status);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
    def do_GET(self):
        path=urlparse(self.path);mode=path.path.split('/')[1];issuer=base+'/'+mode
        if path.path.endswith('/.well-known/openid-configuration'):
            return self.reply({'issuer':issuer,'authorization_endpoint':issuer+'/authorize','token_endpoint':issuer+'/token','jwks_uri':issuer+'/jwks','response_types_supported':['code'],'subject_types_supported':['public'],'id_token_signing_alg_values_supported':['RS256'],'token_endpoint_auth_methods_supported':['none']})
        if path.path.endswith('/jwks'):return self.reply({'keys':[{'kty':'RSA','use':'sig','alg':'RS256','kid':'fixture','n':n,'e':'AQAB'}]})
        if path.path.endswith('/authorize'):
            params={k:v[0] for k,v in parse_qs(path.query).items()};auth[mode]=params
            assert params['code_challenge_method']=='S256' and 'openid' in params['scope'].split()
            if mode=='badstate':
                try:urlopen(params['redirect_uri']+'?'+urlencode({'state':'wrong','code':mode}),timeout=4)
                except Exception as e:assert getattr(e,'code',None)==400
                checks.append('reject wrong state and keep listening')
            self.send_response(302);self.send_header('Location',params['redirect_uri']+'?'+urlencode({'state':params['state'],'code':mode}));self.end_headers();return
        self.reply({},404)
    def do_POST(self):
        mode=self.path.split('/')[1];params={k:v[0] for k,v in parse_qs(self.rfile.read(int(self.headers['Content-Length'])).decode()).items()};expected=auth[mode]
        assert params['code']==mode and params['client_id']=='zork-local-fixture'
        assert params['redirect_uri']==expected['redirect_uri']
        assert b64(hashlib.sha256(params['code_verifier'].encode()).digest())==expected['code_challenge']
        now=int(time.time());claims={'iss':base+'/'+mode,'sub':'local-user','aud':'zork-local-fixture','exp':now+600,'iat':now,'nonce':expected['nonce'],'name':'Local Cue User','email':'fixture@example.test'}
        if mode=='nonce':claims['nonce']='wrong'
        if mode=='issuer':claims['iss']=base+'/wrong'
        if mode=='audience':claims['aud']='another-app'
        if mode=='expired':claims['exp']=now-600
        header=b64(json.dumps({'alg':'RS256','kid':'fixture'}).encode());payload=b64(json.dumps(claims).encode());signed=(header+'.'+payload).encode()
        signature=subprocess.check_output(['openssl','dgst','-sha256','-sign',str(key)],input=signed)
        if mode=='signature':signature=bytes([signature[0]^1])+signature[1:]
        checks.append(mode+' PKCE verified')
        self.reply({'access_token':'local-fixture-only','token_type':'Bearer','expires_in':600,'id_token':signed.decode()+'.'+b64(signature)})
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);base='http://127.0.0.1:'+str(server.server_port)
threading.Thread(target=server.serve_forever,daemon=True).start()
try:
    env=dict(os.environ,ZORK_TEST_OIDC=base,CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_BUILD_JOBS='4')
    subprocess.run(['cargo','test','--locked','-p','zork-gui','--test','cue_account','--','--ignored','--nocapture'],env=env,check=True)
    print('PASS: local signed OIDC fixture; '+', '.join(checks))
finally:server.shutdown()

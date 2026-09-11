#!/usr/bin/env python3
"""Agent-driven MCP management over real isolated Gateway/Mesh, without the MCP CLI."""
import importlib.util
import json
from pathlib import Path
import shutil
import sqlite3
import tempfile
import threading
from http.server import ThreadingHTTPServer

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('mcp_fixture',ROOT/'scripts/test-mcp.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
f=m.f


def main():
    root=Path(tempfile.mkdtemp(prefix='zork-mcp-management-'))
    print('fixture:',root,flush=True)
    nodes=[];http=None;success=False;checks=[]
    try:
        a,b=f.Node(root/'a'),f.Node(root/'b');nodes=[a,b]
        a.pair(b);b.pair(a)
        for node in nodes:
            node.config['admin']={'token':'mcp-fixture'}
            (node.root/'config.json').write_text(json.dumps(node.config))
            node.request=lambda method,path,body=None,n=node:m.request(n,method,path,body)
            node.start()
        for node in nodes:
            f.wait(lambda n=node:n.request('GET','/readyz')[0]==200,'ready')
            f.wait(lambda n=node:n.get('/v1/mesh').get('origin')==n.origin,'Mesh identity')
        session=a.new_task()['session_id'];counter=0
        def ok(result):
            assert result[0] in (200,201,202),result
            return result[1]
        def tool(op,iid=None,**fields):
            nonlocal counter
            counter+=1
            return m.request(a,'POST','/v1/mcp',{'session_id':session,'invocation_id':iid or f'manage-{counter}','request':{'op':op,**fields}})
        setup=ok(tool('setup'))
        assert [v['owner'] for v in setup['targets']]==[a.origin],setup
        assert setup['unavailable_nodes'][0]['reason']=='mcp_management_denied',setup
        http=ThreadingHTTPServer(('127.0.0.1',0),m.HttpMcp)
        threading.Thread(target=http.serve_forever,daemon=True).start()
        config={'name':'agent-installed','transport':{'kind':'http','url':f'http://127.0.0.1:{http.server_port}/mcp'},'grant':{'scope':'mesh'}}
        denied=tool('install',owner=b.origin,config=config)
        assert denied[0]==400 and denied[1]['error']=='mcp_management_denied',denied
        assert not ok(tool('recover'))['pending_delivery']
        with sqlite3.connect(b.root/'state/mcp.sqlite') as db: assert db.execute('SELECT COUNT(*) FROM servers').fetchone()[0]==0
        checks.append('call_grant_does_not_imply_node_management')
        b.config['mesh']['peers'][0]['client']=True
        (b.root/'config.json').write_text(json.dumps(b.config))
        assert len(ok(tool('setup'))['targets'])==2

        def agent_operation(op,**fields):
            # A real Agent session uses its ordinary dynamic call tool registry.
            nonlocal counter
            counter+=1
            before={r['invocation_id'] for r in agent_results()}
            content=json.dumps({'fake_tools':[{'name':'mcp','input':{'op':op,**fields}}]})
            ok(m.request(a,'POST',f'/v1/im/sessions/{session}/messages',{'content':content,'request_id':f'agent-management-{counter}'}))
            def completed():
                for result in agent_results():
                    if result['invocation_id'] not in before:
                        assert result['outcome']=='succeeded',result
                        return result
            return f.wait(completed,'Agent '+op)
        def agent_results():
            results=[]
            for segment in (a.root/'sessions'/session/'segments').glob('*.jsonl'):
                for line in segment.read_text().splitlines():
                    event=json.loads(line).get('event',{})
                    result=event.get('result',{})
                    if event.get('kind')=='tool_result' and result.get('tool')=='mcp': results.append(result)
            return results

        # Compatibility is explicitly loaded; it is absent from new catalogs.
        ok(m.request(a,'POST',f'/v1/im/sessions/{session}/messages',{'content':json.dumps({'fake_tools':[{'name':'tool.help','input':{'tool':'mcp'}}]}),'request_id':'load-mcp-compatibility'}))
        def compatibility_loaded():
            for segment in (a.root/'sessions'/session/'segments').glob('*.jsonl'):
                for line in segment.read_text().splitlines():
                    event=json.loads(line).get('event',{})
                    if event.get('kind')=='tool_result' and event['result']['tool']=='tool.help':
                        assert event['result']['outcome']=='succeeded',event
                        return True
        f.wait(compatibility_loaded,'explicit compatibility help')
        installed_event=agent_operation('install',owner=b.origin,config=config)
        installed=installed_event['data'];ref=installed['server_ref'];revision=installed['config_revision']
        assert ref['owner_origin']==b.origin and len(ref['server_id'])==26
        assert len(ok(tool('installed',owner=b.origin))['items'])==1
        assert ok(tool('install',iid=installed_event['invocation_id'],owner=b.origin,config=config))['operation_id']==installed['operation_id']
        assert agent_operation('probe',server_ref=ref)['data']['items'][0]['name']=='echo'
        checks.append('actual_agent_installs_and_probes_remote_mcp')
        read=ok(tool('configure',server_ref=ref));assert read['config_revision']==revision
        disabled=agent_operation('disable',server_ref=ref,expected_revision=revision)['data']
        assert disabled['availability']=='disabled'
        stale=tool('enable',server_ref=ref,expected_revision=revision)
        assert stale[0]==400 and stale[1]['error']=='mcp_revision_conflict',stale
        enabled=agent_operation('enable',server_ref=ref,expected_revision=disabled['config_revision'])['data']
        shared=agent_operation('share',server_ref=ref,expected_revision=enabled['config_revision'],grant={'scope':'local'})['data']
        assert tool('inspect',server_ref=ref,tool='echo')[0]==400
        assert ok(tool('probe',server_ref=ref))['items'][0]['name']=='echo'
        checks.append('agent_management_cas_and_separate_service_permissions')

        updated_config={**config,'description':'updated by Agent'}
        updated=agent_operation('update',server_ref=ref,expected_revision=shared['config_revision'],config=updated_config)['data']
        # Fault injection models loss of the caller's receipt commit.
        with sqlite3.connect(a.root/'state/mcp.sqlite') as db:
            body={'op':'install','owner':b.origin,'config':config}
            db.execute('UPDATE routes SET call=NULL,request=? WHERE invocation=?',(json.dumps(body),installed_event['invocation_id']))
        a.restart_gateway();b.restart_gateway()
        for node in nodes:f.wait(lambda n=node:n.get('/v1/mesh').get('origin')==n.origin,'Mesh restored')
        recovered=ok(tool('recover'))
        assert recovered['calls'][0]['operation_id']==installed['operation_id'],recovered
        assert len(ok(tool('installed',owner=b.origin))['items'])==1
        assert ok(tool('configure',server_ref=ref))['config']['description']=='updated by Agent'
        removed_event=agent_operation('uninstall',server_ref=ref,expected_revision=updated['config_revision'])
        removed=removed_event['data'];assert removed['availability']=='removed'
        assert ok(tool('uninstall',iid=removed_event['invocation_id'],server_ref=ref,expected_revision=updated['config_revision']))['operation_id']==removed['operation_id']
        assert ok(tool('install',iid=installed_event['invocation_id'],owner=b.origin,config=config))['operation_id']==installed['operation_id']
        assert not ok(tool('installed',owner=b.origin))['items']
        checks.append('restart_and_mutation_receipts_do_not_duplicate_or_resurrect')

        # Bare command and omitted cwd are resolved by the target Gateway.
        script=root/'fixture.py';script.write_text(m.FIXTURE);log=root/'calls.jsonl'
        stdio_config={'name':'stdio-agent','transport':{'kind':'stdio','command':'python3','args':[str(script),str(log)]},'grant':{'scope':'mesh'}}
        stdio=ok(tool('install',owner=b.origin,config=stdio_config))
        effective=ok(tool('configure',server_ref=stdio['server_ref']))['config']['transport']
        assert Path(effective['command']).is_absolute() and str(b.root) in effective['cwd'],effective
        assert ok(tool('probe',server_ref=stdio['server_ref']))['items'][0]['name']=='echo'
        b.config['mesh']['peers'][0]['client']=False
        (b.root/'config.json').write_text(json.dumps(b.config))
        assert tool('configure',server_ref=stdio['server_ref'])[0]==400
        assert tool('probe',server_ref=stdio['server_ref'])[0]==400
        assert tool('inspect',server_ref=stdio['server_ref'],tool='echo')[0]==200
        checks.append('target_runtime_resolution_and_management_revocation')
        print(json.dumps({'checks':checks,'count':len(checks)},indent=2),flush=True)
        success=True
    finally:
        for node in reversed(nodes):node.stop()
        if http:http.shutdown();http.server_close()
        if success:shutil.rmtree(root)

if __name__=='__main__':main()

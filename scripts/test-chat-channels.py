#!/usr/bin/env python3
"""Channel tools and delivery across two real, isolated Mesh nodes with fake models."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sqlite3
import tempfile
import time
import uuid
from urllib.error import HTTPError
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('fixture', ROOT / 'scripts/test-mesh.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)
env_spec = importlib.util.spec_from_file_location('build_env', ROOT / 'scripts/lib/build_env.py')
build_env = importlib.util.module_from_spec(env_spec)
env_spec.loader.exec_module(build_env)
fixture.TARGET = Path(os.environ.get('ZORK_TEST_BIN_DIR',
    str(Path(build_env.build_environment()['CARGO_TARGET_DIR']) / 'debug')))
REPORT = Path(os.environ.get('ZORK_TEST_ARTIFACT_DIR', ROOT / 'artifacts/chat-channels'))
TOKEN = 'isolated-channel-fixture'


def request(node, method, path, body=None, agent=False):
    base = node.agent_url if agent else node.url
    req = Request(base + path, method=method,
        headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + TOKEN},
        data=None if body is None else json.dumps(body).encode())
    try:
        response = urlopen(req, timeout=45)
    except HTTPError as error:
        response = error
    with response:
        raw = response.read()
        return response.status, json.loads(raw) if raw else None


def ok(node, method, path, body=None, agent=False):
    status, value = request(node, method, path, body, agent)
    assert status in (200, 201, 202, 204), (method, path, status, value)
    return value


def sql(node, query, args=()):
    with sqlite3.connect(node.root / 'state/gateway.sqlite') as db:
        return db.execute(query, args).fetchall()


def runtime(node, agent_id):
    return json.loads(sql(node, 'SELECT value FROM node_agents WHERE id=?', (agent_id,))[0][0])['session_id']


def operation(node, caller, tool, args, invocation=None, accepted=True):
    invocation = invocation or 'fixture-' + uuid.uuid4().hex
    value = ok(node, 'POST', '/v1/channels/tools', {
        'session_id': caller, 'invocation_id': invocation, 'tool': tool, 'arguments': args})
    if accepted:
        assert value.get('status') not in ('rejected', 'delivery_unknown'), (tool, value)
    return value


def mailbox(node, session):
    return [json.loads(message['content']) for message in
        ok(node, 'GET', f'/sessions/{session}/messages?limit=200', agent=True)['items']
        if message['role'] == 'mailbox' and message['content'].startswith('{')]


def received(node, session, message):
    return sum(value.get('source') == 'chat' and
        value.get('message', {}).get('message_id') == message for value in mailbox(node, session))


def settled(node, session):
    return ok(node, 'GET', f'/sessions/{session}', agent=True)['status'] in ('idle', 'finished', 'cancelled')


def start(node):
    node.start()
    fixture.wait(lambda: request(node, 'GET', '/readyz')[0] == 200, 'Gateway ready')
    fixture.wait(lambda: ok(node, 'GET', '/v1/node/mesh').get('origin'), 'Mesh ready')


def make_caller(node, identity):
    ok(node, 'POST', '/v1/node/agents', {'id': identity, 'name': identity,
        'role': 'leader', 'profile_id': 'fixture', 'model': 'fixture-model', 'thinking': 'off'})
    home = ok(node, 'POST', f'/v1/node/agents/{identity}/open', {})['session_id']
    path = f'/v1/im/sessions/{home}/messages'
    body = {'content': 'Initialize fixture caller', 'request_id': 'initialize'}
    sent = ok(node, 'POST', path, body)['message']
    assert sent['id'] == f'client-{home}-initialize', sent
    assert ok(node, 'POST', path, body)['message'] == sent
    assert [m for m in ok(node, 'GET', path)['items'] if m['role'] == 'user'] == [sent]
    session = fixture.wait(lambda: runtime(node, identity), 'caller allocated')
    fixture.wait(lambda: settled(node, session), 'caller initialized')
    return session, home


def main():
    REPORT.mkdir(parents=True, exist_ok=True)
    checks = []
    nodes = []

    def passed(name):
        checks.append(name)
        print('PASS: ' + name, flush=True)

    with tempfile.TemporaryDirectory(prefix='zork-channels-') as directory:
        root = Path(directory)
        os.environ['ZORK_REGISTRY_DIR'] = str(root / 'registry')
        try:
            a, b = fixture.Node(root / 'a'), fixture.Node(root / 'b')
            nodes = [a, b]
            for node, other in [(a, b), (b, a)]:
                node.pair(other)
                node.config['admin'] = {'token': TOKEN}
                node.config['mesh']['peers'][0].update(client=True, collaborate=True)
                (node.root / 'config.json').write_text(json.dumps(node.config))
                start(node)
            caller_a, home_a = make_caller(a, 'caller-a')
            caller_b, home_b = make_caller(b, 'caller-b')
            passed('native send, retry and history preserve the core outbox message identity')
            channel = operation(a, caller_a, 'chat.create', {'title': 'Shared channel'})['chat_id']
            assert operation(a, caller_a, 'chat.inspect', {'chat_id': channel})['participants'] == []
            assert not sql(a, 'SELECT 1 FROM product_tasks t JOIN chat_channels c ON c.session_key=t.session_key WHERE c.chat_id=?', (channel,))
            first = operation(a, caller_a, 'chat.send', {'chat_id': channel, 'text': 'Post without subscription'})
            assert operation(a, caller_a, 'chat.preferences', {'chat_id': channel})['subscribed'] is False
            passed('posting needs no membership or subscription; Chat has no Task lifecycle')

            operation(b, caller_b, 'chat.update_preferences', {'target': a.origin, 'chat_id': channel,
                'changes': {'subscribed': True, 'delivery': 'on_next_turn'}})
            assert len(operation(a, caller_a, 'chat.inspect', {'chat_id': channel})['participants']) == 1
            second = operation(a, caller_a, 'chat.send', {'chat_id': channel, 'text': 'Quiet cross-node input'})
            fixture.wait(lambda: received(b, caller_b, second['message_id']) == 1, 'quiet Mesh input')
            assert settled(b, caller_b)
            assert received(b, caller_b, first['message_id']) == 0
            passed('silent remote subscribers are not participants and on_next_turn does not wake them')

            workspace_b = Path(sql(b, 'SELECT workspace_path FROM sessions WHERE id=?', (caller_b,))[0][0])
            path = workspace_b / 'channel-file.txt'
            original = (b'Frozen cross-node file\n' * 3000)
            path.write_bytes(original)
            invocation = 'file-' + uuid.uuid4().hex
            file_args = {'target': a.origin, 'chat_id': channel, 'text': 'Reply with a file',
                'reply_to': first['message_id'], 'attachments': [{'file_path': str(path)}]}
            sent = operation(b, caller_b, 'chat.send', file_args, invocation)
            path.unlink()
            assert operation(b, caller_b, 'chat.send', dict(reversed(list(file_args.items()))), invocation) == sent
            read = operation(b, caller_b, 'chat.read', {'target': a.origin, 'chat_id': channel,
                'attachment_id': sent['attachments'][0]['id']})
            assert Path(read['path']).resolve().is_relative_to(workspace_b.resolve())
            assert Path(read['path']).read_bytes() == original
            assert received(b, caller_b, sent['message_id']) == 0
            participants = operation(a, caller_a, 'chat.inspect', {'chat_id': channel})['participants']
            assert next(p for p in participants if p['author']['id'] == b.origin + '/caller-b')['subscribed']
            passed('remote file send is atomic, immutable and idempotent; sender does not receive its own output')

            local_copy = operation(b, caller_b, 'chat.send', {'chat_id': home_b, 'text': 'Copy from another node',
                'attachments': [{'source_target': a.origin, 'source_chat_id': channel,
                    'attachment_id': sent['attachments'][0]['id']}]})
            copied = operation(b, caller_b, 'chat.read', {'chat_id': home_b,
                'attachment_id': local_copy['attachments'][0]['id']})
            assert Path(copied['path']).read_bytes() == original
            passed('explicit source_target copies a published attachment across nodes')

            operation(b, caller_b, 'chat.update_preferences', {'target': a.origin, 'chat_id': channel,
                'changes': {'filter': 'mentions', 'delivery': 'immediate'}})
            ignored = operation(a, caller_a, 'chat.send', {'chat_id': channel, 'text': 'No mention'})
            mentioned = operation(a, caller_a, 'chat.send', {'chat_id': channel, 'text': 'For the subscriber',
                'mentions': [b.origin + '/caller-b']})
            fixture.wait(lambda: received(b, caller_b, mentioned['message_id']) == 1, 'mentioned input')
            fixture.wait(lambda: settled(b, caller_b), 'mentioned turn finished')
            assert received(b, caller_b, ignored['message_id']) == 0
            passed('per-Agent channel filters control automatic delivery')

            b.stop()
            backlog = operation(a, caller_a, 'chat.send', {'chat_id': channel, 'text': 'While receiver is offline',
                'mentions': [b.origin + '/caller-b']})
            with sqlite3.connect(b.root / 'state/gateway.sqlite') as db:
                # Simulate a crash after durable Agent acceptance but before the
                # Gateway marked its delivery receipt. The source cursor stays committed.
                db.execute("UPDATE chat_mailbox SET delivered=0 WHERE json_extract(notice,'$.message.message_id')=?",
                    (mentioned['message_id'],))
                # Simulate a lost caller-side receipt after remote publication.
                db.execute("UPDATE chat_receipts SET result=NULL WHERE request_key IN (SELECT request_key FROM chat_outgoing WHERE json_extract(value,'$.rpc.invocation_id')=?)",
                    (invocation,))
            start(b)
            fixture.wait(lambda: received(b, caller_b, backlog['message_id']) == 1, 'offline replay')
            fixture.wait(lambda: not sql(b, 'SELECT 1 FROM chat_mailbox WHERE delivered=0'), 'mailbox receipts recovered')
            assert received(b, caller_b, mentioned['message_id']) == 1
            assert operation(b, caller_b, 'chat.recover', {'operation_id': invocation}) == sent
            passed('restart replays offline messages and recovers original receipts without duplicate Agent input')

            operation(b, caller_b, 'chat.update_preferences', {'target': a.origin, 'chat_id': channel,
                'changes': {'subscribed': False}})
            unsubscribed = operation(a, caller_a, 'chat.send', {'chat_id': channel, 'text': 'After unsubscribe',
                'mentions': [b.origin + '/caller-b']})
            assert not sql(a, 'SELECT 1 FROM chat_notices WHERE message_id=?', (unsubscribed['message_id'],))
            participants = operation(a, caller_a, 'chat.inspect', {'chat_id': channel})['participants']
            assert next(p for p in participants if p['author']['id'] == b.origin + '/caller-b')['subscribed'] is False
            passed('unsubscription retains authored participation and mentions cannot bypass it')

            created = operation(b, caller_b, 'agent.create', {'target': a.origin, 'config': {
                'name': 'Independent Agent', 'selection': {'profile_id': 'fixture',
                    'model': 'fixture-model', 'thinking': 'off'}, 'instructions': 'Use the shared channel.'}})
            identity = created['agent']['id']
            assert created['agent']['session_id'] is None
            assert not sql(a, 'SELECT 1 FROM chat_agent_home WHERE agent_id=?', (identity,))
            update_id = 'update-' + uuid.uuid4().hex
            update_args = {'target': a.origin, 'agent_id': identity, 'expected_revision': created['revision'],
                'changes': {'name': 'Updated Agent'}}
            updated = operation(b, caller_b, 'agent.update', update_args, update_id)
            assert operation(b, caller_b, 'agent.update', update_args, update_id) == updated
            conflict = operation(b, caller_b, 'agent.update', dict(update_args, changes={'name': 'Conflict'}), accepted=False)
            assert conflict['status'] == 'rejected' and conflict['error'] == 'agent_configuration_conflict'
            passed('Agent creation is independent of Chats and configuration updates preserve CAS receipts')

            subscribe = json.dumps({'fake_tool': {'name': 'chat.update_preferences', 'input': {
                'chat_id': channel, 'changes': {'subscribed': True}}}})
            operation(b, caller_b, 'agent.message', {'target': a.origin, 'agent_id': identity, 'text': subscribe})
            fixture.wait(lambda: sql(a, 'SELECT 1 FROM chat_preferences WHERE chat_id=? AND agent_ref=?',
                (channel, identity)), 'new Agent subscribes through its own ToolContext')
            assert all(p['author']['id'] != identity for p in operation(a, caller_a, 'chat.inspect',
                {'chat_id': channel})['participants'])
            reply = json.dumps({'fake_tool': {'name': 'chat.send', 'input': {
                'chat_id': channel, 'text': 'New Agent speaks', 'reply_to': first['message_id']}}})
            operation(b, caller_b, 'agent.message', {'target': a.origin, 'agent_id': identity, 'text': reply})
            fixture.wait(lambda: any(p['author']['id'] == identity for p in operation(a, caller_a,
                'chat.inspect', {'chat_id': channel})['participants']), 'actual authored participation')
            passed('direct Agent requests bootstrap its own subscription; only its subsequent post creates participation')

            slow = json.dumps({'fake_tool': {'name': 'shell.run', 'input': {'command': 'sleep 20'}}})
            operation(b, caller_b, 'agent.message', {'target': a.origin, 'agent_id': identity, 'text': slow})
            active = fixture.wait(lambda: next((s for s in operation(b, caller_b, 'agent.inspect',
                {'target': a.origin, 'agent_id': identity})['sessions'] if s['run']), None), 'observed Agent run')
            stopped = operation(b, caller_b, 'agent.interrupt', {'target': a.origin, 'agent_id': identity,
                'session_id': active['session_id'], 'run_id': active['run']['turn_id']})
            assert stopped['cleanup_confirmed'], stopped
            after = operation(a, caller_a, 'chat.send', {'chat_id': channel, 'text': 'Channel remains writable'})
            assert after['status'] == 'committed'
            passed('run-specific interruption waits for cleanup and never closes the Chat')

            dynamic = json.dumps({'fake_tool': {'name': 'chat.send', 'input': {'chat_id': channel,
                'text': 'Dynamic tool pipeline'}}})
            ok(a, 'POST', f'/v1/im/sessions/{home_a}/messages', {'content': dynamic, 'request_id': 'dynamic-channel-send'})
            fixture.wait(lambda: operation(a, caller_a, 'chat.search', {'chat_id': channel,
                'query': 'Dynamic tool pipeline'})['items'], 'registered dynamic channel tool')
            for node in nodes:
                assert all(s['kind'] != 'agent_control' for s in ok(node, 'GET', '/v1/im/sessions')['items'])
            passed('native ingress, registered tools and Mesh consumers share the same business transaction')

            legacy = ok(a, 'POST', '/v1/im/sessions', {'profile_id': 'fixture', 'model': 'fixture-model',
                'thinking': 'off', 'workspace': str(a.workspace)})['session_id']
            # A hidden compatibility tool needs its current contract loaded first.
            help_request = json.dumps({'fake_tool': {'name': 'tool.help', 'input': {'tool': 'chat.post_message'}}})
            ok(a, 'POST', f'/v1/im/sessions/{legacy}/messages', {'content': help_request, 'request_id': 'legacy-help'})
            fixture.wait(lambda: any(m['role'] == 'tool' for m in
                ok(a, 'GET', f'/sessions/{legacy}/messages?limit=200', agent=True)['items']), 'legacy tool contract learned')
            fixture.wait(lambda: settled(a, legacy), 'legacy help turn ends')
            aliases = json.dumps({'fake_tools': [
                {'name': 'chat.post_message', 'input': {'text': 'Legacy visible post', 'kind': 'final'}},
                {'name': 'notify', 'input': {'text': 'PTC self notification'}}]})
            ok(a, 'POST', f'/v1/im/sessions/{legacy}/messages', {'content': aliases, 'request_id': 'legacy-aliases'})
            def aliases_arrived():
                messages = ok(a, 'GET', f'/v1/im/sessions/{legacy}/messages')['items']
                return {m['content'] for m in messages if m['role'] == 'assistant'} == {
                    'Legacy visible post'}
            fixture.wait(aliases_arrived, 'legacy aliases publish through channel transaction')
            fixture.wait(lambda: settled(a, legacy), 'self notification consumed by original Session')
            inputs = [m for m in ok(a, 'GET', f'/sessions/{legacy}/messages?limit=200', agent=True)['items']
                if m['role'] == 'mailbox']
            assert len(inputs) == 3, inputs
            assert sum('summary: PTC self notification' in m['content'] for m in inputs) == 1
            passed('notify targets its own Session mailbox; legacy visible posts remain Chat messages')

            before = ok(a, 'GET', f'/v1/im/sessions/{home_a}/messages')['items']
            binding = ok(a, 'GET', f'/v1/tools/context?threadId={caller_a}')
            ok(a, 'POST', '/notify', {'sessionKey': binding['sessionKey'], 'text': 'Monitor state changed'})
            fixture.wait(lambda: any('summary: Monitor state changed' in m['content'] for m in
                ok(a, 'GET', f'/sessions/{caller_a}/messages?limit=200', agent=True)['items']
                if m['role'] == 'mailbox'), 'monitor event reaches Agent control Session')
            fixture.wait(lambda: settled(a, caller_a), 'monitor notification settles')
            assert ok(a, 'GET', f'/v1/im/sessions/{home_a}/messages')['items'] == before
            passed('background monitor notification wakes its original Agent Session without Chat publication')

        finally:
            for node in nodes:
                node.stop()
                for name in ('bootstrap.log', 'supervisor.log'):
                    path = node.root / name
                    if path.exists():
                        shutil.copyfile(path, REPORT / (node.root.name + '-' + name))
            (REPORT / 'result.json').write_text(json.dumps({'checks': checks,
                'data_removed': True, 'fixture': 'two real local Mesh nodes; fake model; real tools and files'}, indent=2))
    print('PASS: all channel process checks; isolated nodes and test data cleaned', flush=True)


if __name__ == '__main__':
    main()

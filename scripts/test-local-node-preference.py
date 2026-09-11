#!/usr/bin/env python3
"""Real native launches: saved node choice survives failure, quit and restart."""
import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time
from urllib.request import urlopen

spec = importlib.util.spec_from_file_location('ui', Path(__file__).parent / 'lib/native_gui_fixture.py')
ui = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ui)
root = Path(tempfile.mkdtemp(prefix='znp-', dir='/tmp'))
client = root / 'client'
native = ui.Native(None, root)


def preference():
    with sqlite3.connect(client / 'client.db') as db:
        row = db.execute("SELECT value FROM cache WHERE node='device' AND key='local-node-enabled'").fetchone()
        return json.loads(row[0]) if row else None


def start(broken=False):
    env = dict(os.environ, ZORK_CLIENT_DATA=str(client), ZORK_GUI_PREFERENCES_PATH=str(root / 'preferences.json'),
               ZORK_NODE_BINARY=str(root / 'missing' if broken else ui.fixture.TARGET / 'zork'), ZORK_DESKTOP_FAKE_AGENT='1')
    native.process = subprocess.Popen([str(ui.fixture.TARGET / 'zork-gui'), '--dev', '--dev-port',
        native.url.rsplit(':', 1)[1], '--dev-token', 'mesh-native-fixture'], env=env, stdout=native.log, stderr=native.log)
    ui.wait(lambda: native.ui('/health'), 'GUI ready')


def stop():
    native.stop()
    config = client / 'node/config.json'
    if config.exists():
        address = json.loads(config.read_text())['bind']['runtime']
        def gone():
            try:
                urlopen('http://' + address + '/readyz', timeout=.2)
                return False
            except OSError:
                return True
        ui.wait(gone, 'owned node stops with GUI')


try:
    start(broken=True)
    ui.wait(lambda: native.element('local-node-toggle', True), 'first launch off')
    assert preference() is False
    native.click('local-node-toggle')
    ui.wait(lambda: native.element('local-node-retry', True), 'failed start offers retry')
    assert preference() is True
    stop()
    start()
    ui.wait(lambda: native.element('desktop-manage'), 'enabled node starts automatically after failure')
    assert preference() is True
    stop()
    start()
    ui.wait(lambda: native.element('desktop-manage'), 'enabled node starts automatically again')
    assert preference() is True
    stop()
    # Migrate a pre-preference installation that had enabled its local node.
    with sqlite3.connect(client / 'client.db') as db:
        db.execute("DELETE FROM cache WHERE node='device' AND key='local-node-enabled'")
    start()
    ui.wait(lambda: native.element('desktop-manage'), 'previously enabled node migrates to on')
    assert preference() is True
    native.click('desktop-manage')
    ui.wait(lambda: native.element('manage-tab-3'), 'node settings tab')
    native.click('manage-tab-3')
    ui.wait(lambda: native.element('local-node-toggle', True), 'enabled node switch')
    native.click('local-node-toggle')
    ui.wait(lambda: native.element('local-node-toggle', True) and
            native.element('local-node-toggle')['label'] == '开启本机节点', 'explicit off')
    assert preference() is False
    stop()
    start()
    ui.wait(lambda: native.element('local-node-toggle', True), 'explicit off remains off after restart')
    assert preference() is False
    assert not native.element('desktop-manage')
    assert native.element('local-node-toggle')['label'] == '开启本机节点'
    print('PASS: first launch off, failed start preserves on, repeated automatic startup, legacy migration, explicit off survives restart')
finally:
    stop()
    native.log.close()
    print(root)

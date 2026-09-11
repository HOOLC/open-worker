#!/usr/bin/env python3
"""Exercise native version switching without touching user services or real nodes."""
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('ZORK_TEST_BINARY', ROOT / 'target/debug/zork'))


def wait(check, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out waiting for native version switch')


class UpgradeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zu-', dir='/tmp')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        (self.root / 'run').mkdir()
        self.bundle(self.root / 'bin', 'old')
        # A settings fixture, not a launchd/systemd registration.
        (self.root / 'service.json').write_text('{"enabled":true,"start_at_login":false}')
        self.log = (self.root / 'test.log').open('wb')
        self.addCleanup(self.log.close)
        self.process = subprocess.Popen(
            [str(self.root / 'bin/zork'), 'start', '--data', str(self.root)],
            env=dict(os.environ, ZORK_REGISTRY_DIR=str(self.root / 'registry')),
            stdin=subprocess.DEVNULL, stdout=self.log, stderr=self.log,
            start_new_session=True,
        )
        self.addCleanup(self.stop)
        wait(lambda: self.command('status').get('pid') == self.process.pid)
        wait(lambda: len(self.starts()) == 1)

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=12)
            except subprocess.TimeoutExpired:
                os.killpg(self.process.pid, signal.SIGKILL)
                self.process.wait()

    def bundle(self, path, version, invalid=False):
        path.mkdir()
        if invalid:
            (path / 'zork').write_bytes(b'#!/nonexistent-zork-test-interpreter\n')
            (path / 'zork').chmod(0o755)
        else:
            shutil.copy2(BINARY, path / 'zork')
        (path / 'VERSION').write_text(version)
        for name in ['zork-station', 'zork-agent', 'zork-gh']:
            child = path / name
            child.write_text(f'''#!{sys.executable}
import json, os, pathlib, signal, sys
root=pathlib.Path(sys.argv[sys.argv.index('--data')+1])
with (root/'starts.jsonl').open('a') as log:
    log.write(json.dumps({{'name':{name!r},'version':{version!r},'pid':os.getpid()}})+'\\n')
while True: signal.pause()
''')
            child.chmod(0o755)

    def command(self, command):
        with socket.socket(socket.AF_UNIX) as sock:
            sock.settimeout(3)
            sock.connect(str(self.root / 'run/sup.sock'))
            sock.sendall(command.encode() + b'\n')
            line = sock.makefile().readline().strip()
            return json.loads(line) if command == 'status' else line

    def starts(self):
        return [json.loads(line) for line in (self.root / 'starts.jsonl').read_text().splitlines()]

    def test_complete_bundle_switch_preserves_supervisor_and_old_bundle(self):
        self.bundle(self.root / 'run/update-stage', 'new')
        before = self.starts()
        self.assertEqual(self.command('activate-update'), 'accepted')
        after = wait(lambda: (entries if len(entries := self.starts()) >= 2 else None))
        self.assertEqual(self.command('status')['pid'], self.process.pid)
        self.assertEqual({e['version'] for e in after[-1:]}, {'new'})
        self.assertTrue({e['pid'] for e in before}.isdisjoint(e['pid'] for e in after[-1:]))
        self.assertEqual((self.root / 'bin/VERSION').read_text(), 'new')
        backups = list((self.root / 'run').glob('update-previous-*'))
        self.assertEqual(len(backups), 1)
        self.assertEqual((backups[0] / 'VERSION').read_text(), 'old')

    def test_failed_exec_restores_original_bundle(self):
        self.bundle(self.root / 'run/update-stage', 'broken', invalid=True)
        self.assertEqual(self.command('activate-update'), 'accepted')
        after = wait(lambda: (entries if len(entries := self.starts()) >= 2 else None))
        self.assertEqual({e['version'] for e in after[-1:]}, {'old'})
        self.assertEqual((self.root / 'bin/VERSION').read_text(), 'old')
        self.assertEqual(json.loads((self.root / 'run/update.json').read_text())['phase'], 'failed')
        self.assertEqual(self.command('status')['pid'], self.process.pid)

    def test_missing_stage_does_not_stop_children(self):
        self.assertTrue(self.command('activate-update').startswith('error:'))
        self.assertEqual(len(self.starts()), 1)

    def test_foreground_installation_is_not_modified(self):
        self.bundle(self.root / 'run/update-stage', 'new')
        (self.root / 'service.json').write_text('{"enabled":false}')
        self.assertTrue(self.command('activate-update').startswith('error:'))
        self.assertEqual((self.root / 'bin/VERSION').read_text(), 'old')
        self.assertEqual(len(self.starts()), 1)


if __name__ == '__main__':
    unittest.main(verbosity=2)

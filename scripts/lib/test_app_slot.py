"""One owned macOS test-app slot per worktree, shared by packagers."""
from contextlib import contextmanager
import fcntl
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

LSREGISTER = Path('/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister')


def unregister_app(app):
    if sys.platform == 'darwin':
        for bundle in [*app.rglob('*.app'), app]:
            subprocess.run([str(LSREGISTER), '-u', str(bundle)], capture_output=True)


def remove_app(app):
    if not app.exists():
        return
    if app.is_symlink():
        raise RuntimeError(f'Refusing symlink app slot: {app}')
    # Never overwrite/delete an app still used by an independent process.
    if sys.platform == 'darwin':
        check = subprocess.run(['lsof', '-nP', '+D', str(app)], capture_output=True, text=True)
        if check.returncode != 1 or check.stdout.strip() or check.stderr.strip():
            raise RuntimeError(f'App still in use or cannot inspect: {app}; close it before retrying')
    unregister_app(app)
    shutil.rmtree(app)


def app_processes(app):
    prefix = str(app / 'Contents') + '/'
    result = {}
    for line in subprocess.check_output(['ps', '-axo', 'pid=,command='], text=True).splitlines():
        fields = line.strip().split(None, 1)
        if len(fields) == 2 and fields[1].startswith(prefix):
            result[int(fields[0])] = fields[1]
    return result


def stop_app(app):
    running = app_processes(app)
    for pid, command in running.items():
        # Check ownership again immediately before signaling; never quit by bundle ID.
        if app_processes(app).get(pid) == command:
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
    deadline = time.monotonic() + 30
    while app_processes(app):
        if time.monotonic() >= deadline:
            raise RuntimeError(f'Old worktree app did not exit; kept unchanged: {app}')
        time.sleep(.1)
    return bool(running)


class AppSlot:
    def __init__(self, root, name):
        self.root = root
        self.current = root / name
        self.staged = root / '.incoming' / name

    def publish(self, launch=False, restart=True):
        if not self.staged.is_dir() or self.staged.is_symlink():
            raise RuntimeError('No complete staged app to publish')
        old = list(self.root.glob('*.app'))
        if len(old) > 1:
            raise RuntimeError('Multiple legacy slot apps; resolve before updating')
        previous = self.root / '.previous'
        previous.mkdir(exist_ok=True)
        was_running = False
        backup = None
        try:
            if old:
                was_running = stop_app(old[0])
                unregister_app(old[0])
                backup = previous / old[0].name
                old[0].rename(backup)
            try:
                self.staged.rename(self.current)
            except BaseException:
                if backup:
                    backup.rename(old[0])
                    if was_running:
                        subprocess.run(['open', '-n', '-a', str(old[0])], check=True)
                raise
            if backup:
                remove_app(backup)
        finally:
            if not any(previous.iterdir()):
                previous.rmdir()
        if launch or (restart and was_running):
            subprocess.run(['open', '-n', '-a', str(self.current)], check=True)
        return self.current


@contextmanager
def app_slot(repo, name='Zork.app'):
    if name not in ('Zork.app', 'ZorkBrowser.app'):
        raise ValueError('Unsupported app name')
    root = Path(repo) / '.tmp/macos-app.noindex'
    root.mkdir(parents=True, exist_ok=True)
    with (root / 'owner.lock').open('a+') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError('This worktree already has an update/test task; retry after it finishes') from None
        previous = root / '.previous'
        if previous.exists():
            for backup in previous.glob('*.app'):
                if not list(root.glob('*.app')):
                    backup.rename(root / backup.name)
                else:
                    remove_app(backup)
            previous.rmdir()
        incoming = root / '.incoming'
        if incoming.exists():
            for partial in incoming.glob('*.app'):
                remove_app(partial)
            incoming.rmdir()
        incoming.mkdir()
        slot = AppSlot(root, name)
        try:
            yield slot
        finally:
            remove_app(slot.staged)
            if incoming.exists() and not any(incoming.iterdir()):
                incoming.rmdir()


def run_test(command, app):
    """Own only this command's process group; clean up its children on exit."""
    argv = [arg.replace('{app}', str(app)) for arg in command]
    proc = subprocess.Popen(argv, start_new_session=True)
    try:
        code = proc.wait()
        if code:
            raise subprocess.CalledProcessError(code, argv)
    finally:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            try:
                os.killpg(proc.pid, 0)
            except ProcessLookupError:
                break
            time.sleep(.05)
        else:
            os.killpg(proc.pid, signal.SIGKILL)
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()

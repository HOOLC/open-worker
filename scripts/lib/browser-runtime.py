"""Stage the embedded browser built by Cargo; never download on a user's device."""
import json
import os
import re
import plistlib
from pathlib import Path
import shutil
import subprocess
import tempfile
import shlex
import base64
import hashlib
from functools import lru_cache


@lru_cache(maxsize=1)
def signing_identity():
    explicit = os.environ.get('ZORK_CODESIGN_IDENTITY')
    if explicit == '-':
        return explicit
    result = subprocess.run(['security', 'find-identity', '-v', '-p', 'codesigning'],
                            text=True, capture_output=True, check=True)
    identities = re.findall(r'\) ([A-F0-9]{40}) "([^"]+)"', result.stdout)
    certificates = subprocess.check_output(['security', 'find-certificate', '-a', '-p'], text=True)
    certificate_der = {}
    for encoded in re.findall(r'-----BEGIN CERTIFICATE-----(.*?)-----END CERTIFICATE-----', certificates, re.S):
        der = base64.b64decode(encoded)
        certificate_der[hashlib.sha1(der).hexdigest().upper()] = der
    failures = []
    for kind in ('Developer ID Application:', 'Apple Development:'):
        for fingerprint, name in identities:
            if not name.startswith(kind) or (explicit and explicit not in (fingerprint, name)):
                continue
            # Local identity discovery does not check online revocation. A
            # revoked certificate can sign successfully but fail at launch.
            der = certificate_der.get(fingerprint)
            if der is None:
                failures.append(f'{fingerprint}: public certificate unavailable')
                continue
            with tempfile.TemporaryDirectory(prefix='zork-certificate-') as temporary:
                certificate = Path(temporary) / 'signer.cer'
                certificate.write_bytes(der)
                verification = subprocess.run(
                    ['security', 'verify-cert', '-c', str(certificate), '-p', 'codeSign',
                     '-R', 'ocsp', '-R', 'require'], text=True, capture_output=True)
            if verification.returncode == 0:
                return fingerprint
            failures.append(f'{fingerprint}: {(verification.stderr or verification.stdout).strip()}')
    if failures or explicit:
        raise RuntimeError('No trusted signing identity; renew the signing certificate.\n' +
                           '\n'.join(failures or ['Requested identity was not found.']))
    return '-'


def sign(path, identity=None, *, deep=False):
    identity = identity or signing_identity()
    arguments = ['--force', '--sign', identity] + (['--deep'] if deep else []) + [str(Path(path).resolve())]
    if identity == '-':
        subprocess.run(['codesign', *arguments], check=True)
        return
    # Builds can originate in an SSH audit session. Launch Services gives the
    # Apple-signed codesign tool access to the user's normal GUI Keychain session.
    with tempfile.TemporaryDirectory(prefix='zork-sign-') as temporary:
        root = Path(temporary)
        app = root / 'Signer.app'
        mac = app / 'Contents/MacOS'
        mac.mkdir(parents=True)
        executable = mac / 'ZorkSigner'
        executable.write_text('#!/bin/sh\n/usr/bin/codesign "$@" >' + shlex.quote(str(root/'out')) +
                              ' 2>' + shlex.quote(str(root/'err')) + '\nsign_result=$?\nprintf "%s\\n" "$sign_result" >' +
                              shlex.quote(str(root/'result')) + '\nexit "$sign_result"\n')
        executable.chmod(0o755)
        with (app/'Contents/Info.plist').open('wb') as output:
            plistlib.dump({'CFBundleIdentifier':'surf.zork.build-signer', 'CFBundleExecutable':'ZorkSigner',
                          'CFBundleName':'Zork build signer', 'CFBundlePackageType':'APPL', 'LSUIElement':True}, output)
        subprocess.run(['codesign','--force','--sign','-',str(app)],check=True)
        subprocess.run(['open','-n','-W','-g','-a',str(app),'--args',*arguments],check=True)
        if not (root/'result').exists() or (root/'result').read_text().strip() != '0':
            raise RuntimeError((root/'err').read_text() if (root/'err').exists() else 'Signing helper did not complete')


def stage_runtime(binaries: Path, destination: Path):
    executable = binaries / 'zork-browser-runtime'
    helper = binaries / 'zork-browser-helper'
    cef = Path(subprocess.check_output([str(executable), '--cef-dir'], text=True).strip())
    framework = cef / 'Chromium Embedded Framework.framework'
    if not framework.is_dir() or not helper.is_file():
        raise RuntimeError('Build the embedded browser runtime and helper before packaging')
    app = destination / 'ZorkBrowser.app'
    if app.exists():
        raise RuntimeError(f'Browser bundle already exists: {app}')
    contents = app / 'Contents'
    frameworks = contents / 'Frameworks'
    frameworks.mkdir(parents=True)
    # ditto preserves the framework's versioned layout, permissions and links.
    subprocess.run(['ditto', '--norsrc', '--noextattr', str(framework), str(frameworks / framework.name)], check=True)

    def bundle(path, name, source, identifier):
        mac = path / 'Contents/MacOS'
        mac.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, mac / name)
        (mac / name).chmod(0o755)
        with (path / 'Contents/Info.plist').open('wb') as output:
            plistlib.dump({'CFBundleExecutable': name, 'CFBundleName': name,
                          'CFBundleIdentifier': identifier, 'CFBundlePackageType': 'APPL',
                          'CFBundleShortVersionString': '1.0.0', 'CFBundleVersion': '1',
                          'LSUIElement': True, 'LSMinimumSystemVersion': '11.0',
                          'NSSupportsAutomaticGraphicsSwitching': True,
                          'LSEnvironment': {'MallocNanoZone': '0'}}, output)

    bundle(app, 'ZorkBrowser', executable, 'surf.zork.desktop.browser')
    for index, suffix in enumerate(['', ' (Alerts)', ' (GPU)', ' (Plugin)', ' (Renderer)']):
        name = 'ZorkBrowser Helper' + suffix
        bundle(frameworks / (name + '.app'), name, helper, f'surf.zork.desktop.browser.helper{index}')
    resources = contents / 'Resources'
    resources.mkdir(exist_ok=True)
    license = cef / 'LICENSE.txt'
    if license.exists():
        shutil.copyfile(license, resources / 'CEF-LICENSE.txt')
    shutil.copyfile(Path(__file__).resolve().parents[2] / 'crates/zork-browser-runtime/LICENSE.cef-rs', resources / 'cef-rs-LICENSE.txt')
    (resources / 'runtime.json').write_text(json.dumps({'engine': 'CEF', 'binding': '152.0.0+152.0.5'})+'\n')
    subprocess.run(['xattr', '-cr', str(app)], check=True)
    # Seal nested code from the inside out. Signing the unsealed CEF framework
    # indirectly through its parent can fail in the Code Signing subsystem.
    sign(frameworks / framework.name, deep=True)
    for helper_app in sorted(frameworks.glob('*.app')):
        sign(helper_app)
    sign(app)
    subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True)
    return app


if __name__ == '__main__':
    import argparse
    import tarfile
    from test_app_slot import app_slot, run_test
    parser = argparse.ArgumentParser(description='Update the one persistent worktree app slot')
    parser.add_argument('--bin-dir', required=True, type=Path)
    parser.add_argument('--output', type=Path, help='Explicit archive export directory')
    parser.add_argument('--launch', action='store_true')
    parser.add_argument('--run', nargs=argparse.REMAINDER, help='Test command against updated {app}')
    args = parser.parse_args()
    if args.run == []:
        parser.error('--run requires a command')
    repo = Path(__file__).resolve().parents[2]
    with app_slot(repo, name='ZorkBrowser.app') as slot:
        stage_runtime(args.bin_dir.resolve(), slot.staged.parent)
        if args.output:
            args.output.mkdir(parents=True, exist_ok=True)
            archive = args.output / 'ZorkBrowser.tar.gz'
            with tempfile.TemporaryDirectory(prefix='.browser-package-', dir=args.output) as scratch:
                staged = Path(scratch) / archive.name
                with tarfile.open(staged, 'w:gz') as output:
                    output.add(slot.staged, arcname=slot.staged.name)
                os.replace(staged, archive)
        app = slot.publish(launch=args.launch, restart=not bool(args.run))
        if args.run:
            run_test(args.run, app)
        print(app)

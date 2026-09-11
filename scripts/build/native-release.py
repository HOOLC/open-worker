#!/usr/bin/env python3
"""Build and verify GitHub Release assets. Only explicit release binaries are accepted."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
import sys
sys.path.insert(0, str(ROOT / "scripts/lib"))
from build_env import build_environment
COMPONENTS = ('zork', 'zork-gateway', 'zork-agent', 'zork-gh')
PLATFORMS = {'darwin-arm64': '15.0', 'darwin-x64': '15.0',
             'linux-arm64': '2.39', 'linux-x64': '2.39'}
MEMBERS = (*COMPONENTS, 'VERSION', 'LICENSE', 'Synchronicity.txt')


def version():
    value = json.loads((ROOT / 'package.json').read_text())['version']
    if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?', value):
        raise ValueError('Invalid release version')
    return value


def host_platform():
    system = {'Darwin': 'darwin', 'Linux': 'linux'}[platform.system()]
    arch = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'x64'}[platform.machine()]
    return f'{system}-{arch}'


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def verify_archive(archive, release_version):
    with tarfile.open(archive) as tar:
        entries = tar.getmembers()
        if sorted(m.name for m in entries) != sorted(MEMBERS):
            raise ValueError(f'Incomplete or unexpected archive members: {archive}')
        for member in entries:
            if not member.isfile() or member.size == 0:
                raise ValueError(f'Invalid archive member: {member.name}')
            if member.name in COMPONENTS and not member.mode & 0o111:
                raise ValueError(f'Non-executable component: {member.name}')
        if tar.extractfile('VERSION').read().decode().strip() != release_version:
            raise ValueError('Archive version mismatch')


def smoke(sources):
    commands = {'zork': ['capabilities'], 'zork-gateway': ['--help'],
                'zork-agent': ['--help'], 'zork-gh': ['--help']}
    env = {k: v for k, v in os.environ.items() if k not in ('BROKER_API_BASE', 'BROKER_REAL_GH_PATH')}
    with tempfile.TemporaryDirectory(prefix='zork-release-smoke-') as directory:
        for name, args in commands.items():
            data = Path(directory) / name
            if name in ('zork', 'zork-gateway', 'zork-agent'):
                args = [*args, '--data', str(data)]
            result = subprocess.run([str(sources[name].resolve()), *args], env=env,
                                    capture_output=True, text=True, timeout=15)
            expected_status = 1 if name == 'zork-gh' else 0
            if result.returncode != expected_status:
                raise ValueError(f'Packaged {name} failed to run: {result.stderr}')
            if data.exists():
                raise ValueError(f'Packaged {name} wrote node data during its help/capability check')
            if name == 'zork' and json.loads(result.stdout).get('mesh_join') != 1:
                raise ValueError('Packaged Zork does not support mesh enrollment')
            if name == 'zork-gh' and 'BROKER_API_BASE and BROKER_REAL_GH_PATH are required' not in result.stderr:
                raise ValueError('Packaged GitHub helper failed its startup check')


def stage(args):
    key = host_platform()
    release_version = version()
    binaries = Path(build_environment()['CARGO_TARGET_DIR']) / 'release'
    for name in COMPONENTS:
        if not os.access(binaries / name, os.X_OK):
            raise ValueError(f'Missing release binary: {binaries / name}; run cargo build --release')
    args.output.mkdir(parents=True, exist_ok=True)
    sources = {name: binaries / name for name in COMPONENTS}
    sources.update({'LICENSE': ROOT / 'LICENSE',
                    'Synchronicity.txt': ROOT / 'crates/zork-mesh/LICENSE.synchronicity'})
    smoke(sources)
    release_file = args.output / 'VERSION'
    release_file.write_text(release_version + '\n')
    sources['VERSION'] = release_file
    archive = args.output / f'zork-{release_version}-{key}.tar.gz'
    with tarfile.open(archive, 'w:gz') as tar:
        for name in MEMBERS:
            tar.add(sources[name], arcname=name, recursive=False)
    verify_archive(archive, release_version)
    (args.output / f'{key}.tsv').write_text(
        f'{key}\t{archive.name}\t{sha256(archive)}\t{PLATFORMS[key]}\n')
    print(archive)


def assemble(args):
    release_version = version()
    rows = []
    for key in PLATFORMS:
        expected = f'zork-{release_version}-{key}.tar.gz'
        row = (args.output / f'{key}.tsv').read_text().strip().split('\t')
        if row != [key, expected, sha256(args.output / expected), PLATFORMS[key]]:
            raise ValueError(f'Invalid release metadata for {key}')
        verify_archive(args.output / expected, release_version)
        rows.append('\t'.join(row))
    (args.output / 'manifest.tsv').write_text('\n'.join(rows) + '\n')
    (args.output / 'VERSION').write_text(release_version + '\n')
    shutil.copyfile(ROOT / 'scripts/install.sh', args.output / 'install.sh')
    assets = [args.output / f'zork-{release_version}-{key}.tar.gz' for key in PLATFORMS]
    assets += [args.output / name for name in ('manifest.tsv', 'VERSION', 'install.sh')]
    (args.output / 'SHA256SUMS').write_text(''.join(f'{sha256(p)}  {p.name}\n' for p in assets))
    print(f'Verified {len(PLATFORMS)} complete platform packages for v{release_version}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['stage', 'assemble', 'version', 'check-tag'])
    parser.add_argument('--output', type=Path, default=ROOT / 'artifacts/native-release')
    parser.add_argument('--tag')
    args = parser.parse_args()
    if args.action == 'version':
        print(version())
    elif args.action == 'check-tag':
        if args.tag != 'v' + version():
            raise ValueError('Git tag must match package.json version')
    else:
        globals()[args.action](args)


if __name__ == '__main__':
    main()

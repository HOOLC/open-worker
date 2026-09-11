#!/usr/bin/env python3
"""Preview or explicitly clean inactive Cargo targets under ZORK_BUILD_ROOT."""
import argparse
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts/lib'))
from build_env import build_environment


def budget(env):
    high = float(env.get('ZORK_BUILD_BUDGET_GIB') or 100)
    low = float(env.get('ZORK_BUILD_LOW_WATER_GIB') or 80)
    if not (math.isfinite(high) and math.isfinite(low) and 0 < low < high):
        raise ValueError('Require 0 < low-water < budget, both finite')
    return high * 2**30, low * 2**30


def size(path):
    return int(subprocess.check_output(['du', '-sk', str(path)], text=True).split()[0]) * 1024


def candidates(root):
    # Only conventional target roots, never arbitrary source/archive directories.
    choices = [root / 'target', root / 'android']
    isolated = root / 'isolated'
    if isolated.is_dir() and not isolated.is_symlink():
        choices.extend(isolated.iterdir())
    return [p for p in choices if not p.is_symlink() and p.is_dir()
            and (p / '.rustc_info.json').is_file()]


def latest_write(path):
    latest = path.stat().st_mtime
    for directory, _, files in os.walk(path):
        for name in files:
            latest = max(latest, (Path(directory) / name).lstat().st_mtime)
    return latest


def idle(path):
    # Fail closed if process inspection is unavailable or inconclusive.
    if not shutil.which('lsof'):
        raise RuntimeError('lsof is required for cleanup')
    result = subprocess.run(['lsof', '-nP', '+D', str(path)], capture_output=True, text=True)
    if result.returncode not in (0, 1) or result.stderr.strip():
        raise RuntimeError('Cannot reliably inspect open files; cleanup refused')
    return result.returncode == 1 and not result.stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--min-age-hours', type=float, default=24)
    args = parser.parse_args()
    if not math.isfinite(args.min_age_hours) or args.min_age_hours < 0:
        parser.error('min age must be finite and nonnegative')
    env = build_environment()
    if not env.get('ZORK_BUILD_ROOT'):
        parser.error('Set ZORK_BUILD_ROOT to a dedicated cache root first')
    root = Path(env['ZORK_BUILD_ROOT']).expanduser()
    root = (root if root.is_absolute() else ROOT / root).resolve()
    if root in (Path('/'), Path.home(), ROOT.resolve()) or ROOT.resolve().is_relative_to(root):
        parser.error('Build root must be a dedicated cache directory')
    high, low = budget(env)
    if not root.is_dir():
        print(json.dumps({'root': str(root), 'bytes': 0, 'candidates': []})); return
    total = size(root)
    chosen = []
    remaining = total
    if total > high:
        aged = sorted((latest_write(p), p) for p in candidates(root))
        for modified, path in aged:
            if remaining <= low:
                break
            if time.time() - modified < args.min_age_hours * 3600:
                continue
            amount = size(path)
            chosen.append((path, amount))
            remaining -= amount
    print(json.dumps({'root': str(root), 'bytes': total, 'budget_bytes': high,
                      'low_water_bytes': low, 'apply': args.apply,
                      'candidates': [{'path': str(p), 'bytes': n} for p, n in chosen]}, indent=2), flush=True)
    if args.apply:
        for path, _ in chosen:
            if size(root) <= low:
                break
            if path.is_symlink() or not path.resolve().is_relative_to(root):
                raise RuntimeError('Candidate changed; refusing cleanup')
            if time.time() - latest_write(path) < args.min_age_hours * 3600 or not idle(path):
                print('Skipped active/recent target:', path); continue
            subprocess.run(['cargo', 'clean', '--target-dir', str(path)], cwd=ROOT, env=env, check=True)
        print('Remaining bytes:', size(root))


if __name__ == '__main__':
    main()

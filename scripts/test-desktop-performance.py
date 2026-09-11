#!/usr/bin/env python3
"""Native rendering gate: desktop fixtures and 100,000 mixed messages.
"""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts/lib'))
from build_env import build_environment

def run(output, message_count=100000, replays=1):
    env = dict(build_environment(), CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_BUILD_JOBS='4')
    target = Path(env.get('CARGO_TARGET_DIR', ROOT / 'target')).resolve()
    subprocess.run(['cargo', 'build', '--locked', '-p', 'zork-gui',
                    '--features', 'headless-bench,frame-profiler', '--bin', 'zork-gui-render-bench'],
                   cwd=ROOT, env=env, check=True)
    output.mkdir(parents=True, exist_ok=True)
    cases = [(workload,100,0) for workload in ('history','chat','markdown')]
    cases += [('mixed',anchor,replay) for replay in range(replays)
              for anchor in (0,message_count//2,max(0,message_count-40))]
    cases.append(("files",0,0))
    failures = []
    for workload,anchor,replay in cases:
        case_output = output / f'replay-{replay}' / f'mixed-{anchor}' if workload=='mixed' else output
        case_env = dict(env,ZORK_BENCH_ANCHOR=str(anchor),ZORK_BENCH_MESSAGE_COUNT=str(message_count))
        result = subprocess.run(['caffeinate', '-d', '-i', '-u', str(target / 'debug/zork-gui-render-bench'),
                                 '--native', workload, str(case_output)], cwd=ROOT, env=case_env)
        if result.returncode:
            failures.append(f'{workload}, replay {replay}, anchor {anchor}')
    if failures:
        raise SystemExit('Desktop performance gate failed: ' + '; '.join(failures))
    print('Current desktop rendering gate passed; native and headless use the same fixtures.', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--messages', type=int, default=100000, help='Mixed-message stress count (default: 100000)')
    parser.add_argument('--replays', type=int, default=1, help='Repeat each mixed-message region (default: 1)')
    parser.add_argument('--output', type=Path, help='Keep JSON measurements in this directory')
    args = parser.parse_args()
    if not 100 <= args.messages <= 1000000:
        parser.error('--messages must be between 100 and 1000000')
    if not 1 <= args.replays <= 10:
        parser.error('--replays must be between 1 and 10')
    selected_run = lambda output: run(output,args.messages,args.replays)
    if args.output:
        selected_run(args.output.resolve())
    else:
        with tempfile.TemporaryDirectory(prefix='zork-performance-') as directory:
            selected_run(Path(directory))

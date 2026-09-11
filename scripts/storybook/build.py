#!/usr/bin/env python3
"""Build native evidence, historical design references and the shared GPUI Web gallery."""
import argparse, os, subprocess, sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]

sys.path.insert(0, str(ROOT / 'scripts/lib'))
from build_env import build_environment

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,default=ROOT/'apps/zork-design/components')
    p.add_argument('--base',default='http://127.0.0.1:49186')
    p.add_argument('--dev-reference',action='store_true',help='Use the running Vite+ reference adapter')
    p.add_argument('--skip-web-build',action='store_true',help='Reuse an already verified WASM build')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    env=dict(build_environment(),CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_BUILD_JOBS='4')
    commands=[
        [sys.executable,str(ROOT/'scripts/storybook/test_package.py')],
        ['cargo','run','--locked','-p','zork-gui','--features','headless-bench','--bin','zork-gui-storybook','--','--export',str(args.output)],
        ['uv','run',str(ROOT/'scripts/storybook/capture_design.py'),str(args.output),'--base',args.base]+(['--dev-reference'] if args.dev_reference else []),
    ]
    if not args.skip_web_build:commands.append([sys.executable,str(ROOT/'scripts/storybook/build_web.py'),'--output',str(args.output/'web')])
    commands.append([sys.executable,str(ROOT/'scripts/storybook/build_gallery.py'),str(args.output)])
    for command in commands:subprocess.run(command,cwd=ROOT,env=env,check=True)
    print('Component gallery ready:',args.output/'index.html')

if __name__=='__main__':main()

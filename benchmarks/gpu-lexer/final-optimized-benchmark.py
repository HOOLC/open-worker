"""Two native runs around fresh browser runs; never accept competing builds/tests."""
import json,os,subprocess
from pathlib import Path
root=Path(__file__).resolve().parent
base={k:v for k,v in os.environ.items() if not k.startswith(('LEXER_','ZORK_WGPU_'))}
base.update(LEXER_BACKEND='optimized',LEXER_MANIFEST='fixtures/sweep-manifest.json',LEXER_REPS='100',LEXER_GPU_ONLY='1',LEXER_START_ROUND='2')
def native(round):
    directory=root/f'results/optimization/final-{round}'
    attempt=0
    while True:
        attempt+=1
        subprocess.run(['python3',str(root/'run.py')],env={**base,'LEXER_RESULTS_DIR':str(directory)},check=True)
        if not json.loads((directory/'overlap-2.json').read_text()):break
        directory.rename(directory.with_name(directory.name+f'-discarded-{attempt}'))
    print('Native final run',round,'complete',flush=True)
native(1)
subprocess.run(['uv','run','--script',str(root/'benchmark-browser.py')],env={**base,'LEXER_RESULT_PREFIX':'final-optimized-browser','LEXER_CONFIRM_REPS':'30'},check=True)
native(2)

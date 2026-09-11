"""Run native candidates serially and retain only uncontended timing runs."""
import os,json,subprocess
from pathlib import Path
root=Path(__file__).resolve().parent
base={k:v for k,v in os.environ.items() if not k.startswith(('LEXER_','ZORK_WGPU_'))}
base.update(LEXER_BACKEND='tint',LEXER_MANIFEST='fixtures/sweep-manifest.json',LEXER_REPS='100',LEXER_GPU_ONLY='1',LEXER_START_ROUND='2')
for mode,private in [('shared',None),('weights','weights'),('private','all')]:
    directory=root/f'results/optimization/select-{mode}'
    attempt=0
    while True:
        attempt+=1
        env={**base,'LEXER_RESULTS_DIR':str(directory)}
        if private:env['LEXER_METAL_PRIVATE']=private
        subprocess.run(['python3',str(root/'run.py')],env=env,check=True)
        if not json.loads((directory/'overlap-2.json').read_text()):break
        directory.rename(directory.with_name(directory.name+f'-discarded-{attempt}'))
    print('Completed candidate',mode,flush=True)
env={**base,'LEXER_RESULT_PREFIX':'optimization-browser','LEXER_CONFIRM_REPS':'30'}
subprocess.run(['uv','run','--script',str(root/'benchmark-browser.py')],env=env,check=True)

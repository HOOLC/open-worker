"""Build first; then run measurements without compiler/process-test competition."""
from pathlib import Path
import json, os, subprocess, time
root=Path(__file__).resolve().parent
binary=Path(os.environ['CARGO_TARGET_DIR'])/'release/zork-gpu-lexer-bench'
def busy():
    text=subprocess.check_output(['ps','-axo','comm='],text=True)
    return [line for line in text.splitlines() if Path(line.strip()).name in {'rustc','cargo','clang','clang++','zork-gui-render-bench'}]
while busy():
    time.sleep(2)
results=root/os.environ.get('LEXER_RESULTS_DIR','results')
results.mkdir(parents=True,exist_ok=True)
meta={'date':subprocess.check_output(['date','-u'],text=True).strip(),'system':subprocess.check_output(['sw_vers'],text=True),'cpu':subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip(),'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'source_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'binary_sha256':subprocess.check_output(['shasum','-a','256',str(binary)],text=True).split()[0]}
(results/'environment.json').write_text(json.dumps(meta,indent=2)+'\n')
for round in range(int(os.environ.get('LEXER_START_ROUND','1')),3):
    while busy(): time.sleep(2)
    (results/f'processes-before-{round}.txt').write_text(subprocess.check_output(['ps','-axo','pid,pcpu,comm'],text=True))
    overlaps=[]
    with (results/f'run-{round}.json').open('w') as output, (results/f'run-{round}.log').open('w') as log:
        process=subprocess.Popen([str(binary)],stdout=output,stderr=log)
        while process.poll() is None:
            competitors=busy()
            if competitors: overlaps.append(competitors)
            time.sleep(0.5)
        if process.returncode: raise SystemExit(process.returncode)
    (results/f'overlap-{round}.json').write_text(json.dumps(overlaps)+'\n')
    if overlaps: print(f'Round {round} has compiler/test overlap; do not use it as clean measurement',flush=True)
    else: print(f'Round {round} complete without compiler/test overlap',flush=True)

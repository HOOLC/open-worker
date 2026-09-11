from pathlib import Path
import json, hashlib
root=Path(__file__).resolve().parent
repo=root.parent.parent
out=[]
sources=[('rust','rs','crates/zork-ui/src/components/message_code.rs'),('typescript','js','scripts/storybook/capture_design.py'),('json','json','package.json'),('yaml','yaml','.github/workflows/ci.yml'),('python','py','scripts/lib/build_env.py')]
# Real TS fixture, selected from repository rather than relabeling another language.
ts=next(p for p in (repo/'apps/zork-design/src').rglob('*.ts') if 'node_modules' not in p.parts and p.stat().st_size>3000)
sources[1]=('typescript','js',str(ts.relative_to(repo)))
for name,lang,path in sources:
    source=(repo/path).read_text()
    if name=='typescript': lang='ts'
    for target in [512,4096,16384,61440,262144]:
        lines=[];size=0
        while size<target:
            for line in source.splitlines(keepends=True):
                if size>=target:break
                lines.append(line);size+=len(line.encode())
        text=''.join(lines);file=f'{name}-{target}.txt';(root/'fixtures'/file).write_text(text)
        out.append(dict(file=file,language=lang,source=path,sha256=hashlib.sha256(text.encode()).hexdigest(),construction='whole-line prefix / repetition of repository source; may end mid-syntax'))
(root/'fixtures/utf8.txt').write_text('let 你好🐈 = "café\\n";\r\n\t// 中文 🦀\nαβ::thing::<T>();\n/* x */ a??b => ${{x}};\\\n')
(root/'fixtures/manifest.json').write_text(json.dumps(out,indent=2)+'\n')

"""Report the byte-doubling benchmark without rerunning measurements."""
from pathlib import Path
import json
root=Path(__file__).resolve().parent
browser=[json.loads((root/f'results/sweep-browser-{i}.json').read_text()) for i in [1,2]]
native=json.loads((root/'results/sweep-native/run-2.json').read_text())
assert json.loads((root/'results/sweep-native/overlap-2.json').read_text())==[]
assert all(not b['overlaps'] and len(b['results'])==55 for b in browser)
assert len(native['results'])==55
web=[{r['file']:r for r in b['results']} for b in browser]
cpu={r['file']:r for r in native['results']}
names=['rust','typescript','json','yaml','python']
sizes=[2**i for i in range(3,14)]
lines=['# 原版 WebGPU：8B 至 8KiB 翻倍测量', '', '2026-09-09，同一台 Apple M4 / 10 核 GPU / macOS 26.5，Chrome '+browser[0]['browser']+'。未修改 gpu-lexer@0.0.2，独立 worker 内直接计时 `await parse(code)`。', '', '## 方法', '', '- 精确 8、16、32、64、128、256、512、1024、2048、4096、8192 字节，5 种语言，共 55 个样本。', '- 从上次固定的 16KiB 样本取前缀；UTF-8 截断点不足的字节补空格，补齐量记在 manifest。输入可不完整，极短前缀可能仅包含注释或声明。这不是跨代码库代表性语料。', '- 原版浏览器主测每档 100 次、另一独立浏览器复测 30 次；第一遍 parse 单独记录，表格是预热后的中位数。', '- 原生基线每档 100 次，syntect 与此前 Rust/Metal 移植交替计时；不与浏览器同时运行。未计结果缓存命中或渲染。', '- 三份测量记录均未检测到编译/性能测试竞争。原版计时包括分词、调度、GPU 上传/计算/回读和 span 合并，排除 worker/Playwright 消息传输。', '', '## 原版 WebGPU 中位数（ms）', '', '| 输入字节 | Rust | TypeScript | JSON | YAML | Python |', '|---:|---:|---:|---:|---:|---:|']
for size in sizes:
 values=[web[0][f'sweep/{n}-{size}.txt']['median_ms'] for n in names]
 lines.append('| '+str(size)+' | '+' | '.join(f'{v:.3f}' for v in values)+' |')
lines+=['', '## 原版 WebGPU p95（ms）', '', '| 输入字节 | Rust | TypeScript | JSON | YAML | Python |', '|---:|---:|---:|---:|---:|---:|']
for size in sizes:
 lines.append('| '+str(size)+' | '+' | '.join(f"{web[0][f'sweep/{n}-{size}.txt']['p95_ms']:.3f}" for n in names)+' |')
lines+=['', '## syntect 中位数（ms）', '', '| 输入字节 | Rust | TypeScript | JSON | YAML | Python |', '|---:|---:|---:|---:|---:|---:|']
for size in sizes:
 values=[cpu[f'sweep/{n}-{size}.txt']['syntect'] for n in names]
 lines.append('| '+str(size)+' | '+' | '.join(f"{v['median_ms']:.3f}" if v else '不支持' for v in values)+' |')
lines+=['', '## 交叉点', '', '以下仅是这些前缀样本中，两轮原版中位数都低于当前 syntect 基线的最小测试档位，不能直接作为生产路由阈值。', '']
for name in names:
 matches=[]
 for size in sizes:
  key=f'sweep/{name}-{size}.txt';s=cpu[key]['syntect']
  if s and all(w[key]['median_ms']<s['median_ms'] for w in web):matches.append(size)
 if name=='typescript':lines.append('- TypeScript：当前 syntect 不支持，无法比较。')
 else:lines.append(f"- {name}："+(str(matches[0])+' B。' if matches else '8–8192 B 中无两轮均胜出的档位。'))
lines+=['', '## 原版复测中位数（ms）', '', '| 输入字节 | Rust | TypeScript | JSON | YAML | Python |', '|---:|---:|---:|---:|---:|---:|']
for size in sizes:
 lines.append('| '+str(size)+' | '+' | '.join(f"{web[1][f'sweep/{n}-{size}.txt']['median_ms']:.3f}" for n in names)+' |')
lines+=['', '## 复现与产物', '', '样本清单：`fixtures/sweep-manifest.json`。原始数据：`results/sweep-browser-{1,2}.json`、`results/sweep-native/run-2.json`，保留每档 p95、首次调用、硬件信息和竞争监控。', '', '```sh', 'python3 benchmarks/gpu-lexer/make-sweep.py', 'LEXER_MANIFEST=fixtures/sweep-manifest.json LEXER_RESULT_PREFIX=sweep-browser LEXER_REPS=100 LEXER_CONFIRM_REPS=30 uv run --script benchmarks/gpu-lexer/benchmark-browser.py', '# Native build setup is documented in README.md.', 'LEXER_MANIFEST=fixtures/sweep-manifest.json LEXER_RESULTS_DIR=results/sweep-native LEXER_REPS=100 LEXER_START_ROUND=2 python3 benchmarks/gpu-lexer/run.py', 'python3 benchmarks/gpu-lexer/report-sweep.py', '```', '', '生产高亮未修改。未测 WebGL、界面帧率、功耗或峰值内存。']
(root/'SWEEP-REPORT.md').write_text('\n'.join(lines)+'\n')

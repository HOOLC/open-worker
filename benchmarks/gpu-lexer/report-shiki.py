"""Add both Shiki engines to the existing same-input performance comparison."""
from pathlib import Path
import json,statistics
root=Path(__file__).resolve().parent
results=root/'results'
shiki={}
for engine in ['oniguruma','javascript']:
 shiki[engine]=[]
 for i in [1,2]:
  data=json.loads((results/f'shiki/{engine}-{i}.json').read_text())
  assert not data['overlaps'] and len(data['results'])==55 and data['init']['version']=='4.4.3'
  shiki[engine].append(data)
look={k:[{r['file']:r for r in run['results']} for run in v] for k,v in shiki.items()}
old={r['file']:r for r in json.loads((results/'sweep-native/run-2.json').read_text())['results']}
ml={r['file']:r for r in json.loads((results/'optimization/final-1/run-2.json').read_text())['results']}
web={r['file']:r for r in json.loads((results/'final-optimized-browser-1.json').read_text())['results']}
sizes=[2**i for i in range(3,14)]
lines=['# Shiki 与其它高亮器的同尺寸比较', '', 'Shiki 4.4.3，默认 Oniguruma/WASM 引擎与 JavaScript RegExp 引擎均测试。结论：这批短前缀上 Shiki 更快；较大的 Rust、TypeScript、YAML、Python 上优化 ML 更快；JSON 到 8KiB 仍是 Shiki 更快。', '', '## 统一摘要', '', '单位 ms，越低越好。每格是 Rust、JSON、YAML、Python 四种样本各自中位数的平均值。TypeScript 单列，避免把 syntect 不支持的语言计入平均。Shiki 为本轮实测，其余列引用先前有效测量；不是五个引擎同进程交替测试。', '', '| 字节 | syntect | Shiki Oniguruma | Shiki JS | 优化 Rust ML | 原版 WebGPU ML |', '|---:|---:|---:|---:|---:|---:|']
for size in sizes:
 keys=[f'sweep/{lang}-{size}.txt' for lang in ['rust','json','yaml','python']]
 values=[statistics.mean(old[k]['syntect']['median_ms'] for k in keys),statistics.mean(look['oniguruma'][0][k]['median_ms'] for k in keys),statistics.mean(look['javascript'][0][k]['median_ms'] for k in keys),statistics.mean(ml[k]['gpu']['median_ms'] for k in keys),statistics.mean(web[k]['median_ms'] for k in keys)]
 lines.append('| '+str(size)+' | '+' | '.join(f'{v:.3f}' for v in values)+' |')
lines+=['', '## 8KiB 各语言', '', '| 语言 | Shiki Oniguruma | Shiki JS | 优化 Rust ML |','|---|---:|---:|---:|']
for lang in ['rust','typescript','json','yaml','python']:
 key=f'sweep/{lang}-8192.txt'
 lines.append(f"| {lang} | {look['oniguruma'][0][key]['median_ms']:.3f} | {look['javascript'][0][key]['median_ms']:.3f} | {ml[key]['gpu']['median_ms']:.3f} |")
lines+=['', '## 方法', '', '- 同一台 Apple M4 / 10 核 GPU 主机，macOS 26.5，Chrome '+shiki['oniguruma'][0]['browser']+'。Shiki 实际使用 CPU 的 WASM 或 JS 正则引擎，不使用 GPU。', '- 11 个精确字节档位 × 5 种语言，沿用已有 55 个固定代码前缀。主测每档 100 次；另一新浏览器进程复测 30 次。复测反转两种引擎的执行顺序。全部四轮未检测到编译或渲染基准竞争。', '- 依照 [Shiki 性能建议](https://shiki.style/guide/best-performance)，使用精简导入，在独立 Worker 中创建并复用一个 highlighter，只加载五种语言和 github-light 主题。', '- 调用 `codeToTokens`，再转为全局 UTF-16 区间，填补换行区间并合并相邻相同颜色/字体样式；这部分转换计入时间。没有计入 HTML 生成、DOM/GPUI 渲染、初始化或 worker IPC。', '- 每次从初始 grammar state 开始重新高亮，没有代码结果缓存。tokenizeTimeLimit=0、tokenizeMaxLineLength=0，关闭解析时间/行长度截断；未设置 forgiving 降级。', '- 计时后验证所有 token 内容与源文本一致，以及扁平区间连续覆盖整个输入。未验证不同引擎的颜色完全一致。', '- 浏览器开启跨源隔离。极短输入耗时接近 performance.now 的约 5 微秒粒度，个位微秒的差别不宜过度解释。', '- Shiki 使用自己的 TextMate 语法库和 github-light；syntect 使用其自带语法库、regex-fancy 与 InspiredGitHub。差异不能只归因于 Rust vs JS。', '- [引擎说明](https://shiki.style/guide/regex-engines)：JS 引擎不保证所有语言都更快；本次 TypeScript 在 JS 引擎上明显慢于 Oniguruma。', '', '## 初始化', '', '单位 ms，首次 parse 另列，可能包含规则/正则的惰性编译。模块导入通过 localhost，本表不代表网络首屏。', '', '| 引擎 / 轮次 | 模块导入 | highlighter 初始化 | 首个 8B Rust 高亮 |', '|---|---:|---:|---:|']
for engine,runs in shiki.items():
 for i,run in enumerate(runs,1):
  lines.append(f"| {engine} / {i} | {run['init']['import_ms']:.2f} | {run['init']['init_ms']:.2f} | {run['results'][0]['first_ms']:.2f} |")
lines+=['', '## 每语言中位数 / p95', '', '所有数字单位 ms；复测列为中位数。']
for lang in ['rust','typescript','json','yaml','python']:
 lines+=['',f'### {lang}', '', '| 字节 | Oniguruma median / p95 | JS median / p95 | Oniguruma 复测 | JS 复测 |','|---:|---:|---:|---:|---:|']
 for size in sizes:
  key=f'sweep/{lang}-{size}.txt';a=look['oniguruma'][0][key];b=look['javascript'][0][key]
  lines.append(f"| {size} | {a['median_ms']:.3f} / {a['p95_ms']:.3f} | {b['median_ms']:.3f} / {b['p95_ms']:.3f} | {look['oniguruma'][1][key]['median_ms']:.3f} | {look['javascript'][1][key]['median_ms']:.3f} |")
lines+=['', '## 复现', '', '```sh', 'npx --yes pnpm@10.33.0 --dir benchmarks/gpu-lexer/shiki install --ignore-workspace --frozen-lockfile --ignore-scripts', 'node benchmarks/gpu-lexer/shiki/build.mjs', 'uv run --script benchmarks/gpu-lexer/benchmark-shiki.py', 'python3 benchmarks/gpu-lexer/report-shiki.py', '```', '', '独立依赖与锁文件位于 shiki/，未改根项目依赖。原始数据位于 results/shiki/，包含所有计时样本、p95、首次调用、版本、bundle SHA-256 和竞争监控。浏览器与本地 HTTP server 已关闭。']
(root/'SHIKI-REPORT.md').write_text('\n'.join(lines)+'\n')

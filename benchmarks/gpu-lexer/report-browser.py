"""Compare unchanged browser API against recorded native measurements."""
from pathlib import Path
import json
root=Path(__file__).resolve().parent
native=json.loads((root/'results/run-1.json').read_text())
browsers=[json.loads((root/f'results/browser-{i}.json').read_text()) for i in [1,2]]
assert all(not b['overlaps'] and len(b['results'])==25 for b in browsers)
lookup={r['file']:r for r in native['results']}
lines=['# 原版 JS / WebGPU 实测', '', '原版是 WebGPU，不是 WebGL。本次直接调用固定 npm 包 gpu-lexer@0.0.2 的公开 `parse`，没有修改其源码、WGSL 或模型。', '', '**原版显著快于当前 Rust/Metal 移植。此前关于 JSON 的负面 GPU 结论只适用于那份移植；原版对本次 4–256KiB JSON 样本也快于 syntect。不能从这次差距推导 Rust 语言或 wgpu 必然更慢，原因尚未定位。**', '', '## 环境与范围', '', f"- 同一台 Apple M4 / 10 核 GPU / macOS 26.5；Chrome for Testing {browsers[0]['browser']}，headless，独立 module worker。", '- WebGPU adapter vendor=apple、architecture=metal-3、isFallbackAdapter=false，支持 shader-f16；CDP 报告 Apple M4 硬件。没有使用软件 GPU。', '- 相同 25 个固定样本。两个独立浏览器进程，主测量 30 次、复测 10 次，均无检测到的编译或性能测试竞争。', '- worker 内 `performance.now()` 围绕 `await parse(code)` 计时：包含原版分词、内部任务调度、GPU 上传/计算/回读、span 合并。验证与 Playwright / worker 消息传输在计时外；不包含 DOM/GPUI 渲染。', '- 直接返回原版 UTF-16 span，未追加 Rust 所需的 UTF-8 转换。重复调用会重新计算高亮，原版复用其缓冲区。', '- syntect 与 Rust/Metal 列引用本次任务此前的有效原生主测量，未在浏览器测量期间并行运行。不是同一进程交替运行三个引擎。', '', '## 约 60KiB 结果', '', '单位 ms，主测量中位数。', '', '| 语言 | syntect | Rust/Metal 移植 | 原版 WebGPU | 原版相对 syntect |','|---|---:|---:|---:|---:|']
for r in browsers[0]['results']:
 if '61440' not in r['file']: continue
 n=lookup[r['file']];s=n['syntect'];web=r['median_ms']
 st=f"{s['median_ms']:.2f}" if s else '不支持'
 ratio=f"{s['median_ms']/web:.2f}×" if s else '—'
 lines.append(f"| {r['language']} | {st} | {n['gpu']['median_ms']:.2f} | {web:.2f} | {ratio} |")
lines+=['', '## 全部样本及复测', '', '| 样本 | 原版主测 median / p95 ms | 原版复测 median ms | Rust median ms | syntect median ms |', '|---|---:|---:|---:|---:|']
for a,b in zip(browsers[0]['results'],browsers[1]['results']):
 assert a['file']==b['file']
 n=lookup[a['file']];st=f"{n['syntect']['median_ms']:.2f}" if n['syntect'] else '不支持'
 lines.append(f"| {a['file']} | {a['median_ms']:.2f} / {a['p95_ms']:.2f} | {b['median_ms']:.2f} | {n['gpu']['median_ms']:.2f} | {st} |")
lines+=['', '## 首次调用', '', '| 轮次 | 模块 import ms | 第一次 parse ms |', '|---|---:|---:|']
for i,b in enumerate(browsers,1):lines.append(f"| {i} | {b['worker_info']['import_ms']:.2f} | {b['results'][0]['first_ms']:.2f} |")
lines+=['', '每次新浏览器、新 worker，第一次 parse 包含公开 API 的惰性初始化。此前为记录硬件信息已 requestAdapter；OS shader cache 未清空，不代表 GPU 服务完全冷启动或首次安装后的性能。', '', '## 判断', '', '- 原版在这些约 60KiB 的 Rust/YAML/Python 上比 syntect 快约 14–21 倍；JSON 也快约 2.1 倍。', '- 短片段仍应按语言看：约 0.5KiB JSON、YAML、Rust 是 syntect 更快；Python 的原版 WebGPU 在两轮中略快于这里的 syntect 基线，不能再笼统断言短代码块全部不适合 GPU。', '- 当前 Rust/Metal 移植相同模型输出通过对照，但速度不能代表原版；原生接入前值得专门定位 shader 编译、dispatch、同步/回读等阶段的差距，尚不能归因于某一项。', '- 上述为高亮计算收益，未验证用户界面的帧率、功耗、内存峰值或 WebGL 后端。原版没有 WebGL 后端，本次未实现它。', '', '## 复现', '', '从仓库根目录运行：', '', '```sh', 'uv run --script benchmarks/gpu-lexer/benchmark-browser.py', 'python3 benchmarks/gpu-lexer/report-browser.py', '```', '', '脚本自动关闭其浏览器和本地 HTTP server。数据在 `results/browser-1.json`、`results/browser-2.json`，包括所有计时样本、硬件信息和竞争检测。']
(root/'BROWSER-REPORT.md').write_text('\n'.join(lines)+'\n')

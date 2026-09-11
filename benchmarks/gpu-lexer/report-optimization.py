"""Summarize the final optimized-native vs unchanged-browser validation."""
from pathlib import Path
import json,statistics
root=Path(__file__).resolve().parent
r=root/'results'
native=[json.loads((r/f'optimization/final-{i}/run-2.json').read_text()) for i in [1,2]]
web=[json.loads((r/f'final-optimized-browser-{i}.json').read_text()) for i in [1,2]]
for i in [1,2]:assert not json.loads((r/f'optimization/final-{i}/overlap-2.json').read_text())
assert all(not x['overlaps'] for x in web)
assert all(len(x['results'])==55 for x in native+web)
n=[{x['file']:x for x in run['results']} for run in native]
w=[{x['file']:x for x in run['results']} for run in web]
old={x['file']:x for x in json.loads((r/'sweep-native/run-2.json').read_text())['results']}
keys=list(n[0]);wins=sum(n[0][k]['gpu']['median_ms']<w[0][k]['median_ms'] for k in keys)
robust=sum(max(x[k]['gpu']['median_ms'] for x in n)<min(x[k]['median_ms'] for x in w) for k in keys)
verification=json.loads((r/'optimization/verification.json').read_text())
assert not verification['mismatches']
lines=['# Rust GPU 优化结果', '',f'主测中，优化版在 **{wins}/55** 个样本上快于未修改的原版 WebGPU。按“较慢的原生轮次仍快于较快的浏览器轮次”计算，**{robust}/55** 个样本保持优势。', '', '## 实现', '', '- 新增 `LEXER_BACKEND=optimized`：运行时为 Rust + 原生 Metal，没有 JS 或浏览器依赖。', '- 使用原版 gpu-lexer 0.0.2 经 Chrome/Dawn/Tint 生成的七个 Metal 内核；只规范化入口名称，保留模型权重、f16 精度、范围保护和工作组初始化。', '- 通过 MTLSharedEvent 等待 GPU 输出就绪，有 60 秒超时；不使用 1ms 轮询或 CPU 忙等，也不再经过完成回调与 Rust channel 的额外通知路径。', '- CPU 输入/输出使用一致性共享内存，权重及 GPU 临时张量使用私有内存。按原版思路保留几何增长的缓冲区容量，缓存形状元数据。', '- 使用串行 compute encoder 和受跟踪资源提供阶段间顺序，与 Dawn 相同。每次重新执行完整模型，不缓存分类结果、不跳过高亮、不降低精度。', '- 原来的 wgpu/Naga 实现保留为默认比较基线。尝试过的内核融合、共享内存布局、循环和编译参数实验未接入最终优化路径；相应记录保留在 experiments 和 results/optimization。', '', '## 三方/四方性能', '', '单位 ms。每格是 Rust、JSON、YAML、Python 四种固定前缀样本各自中位数的平均值。TypeScript 因 syntect 不支持而不参与平均，下方单列。优化版及浏览器来自本轮新测量，旧 Rust GPU 与 syntect 引用前一轮有效基线。', '', '| 字节 | syntect | 旧 Rust GPU | 优化 Rust GPU | 原版 WebGPU | 原版/优化版 |','|---:|---:|---:|---:|---:|---:|']
sizes=[2**i for i in range(3,14)]
for size in sizes:
 ks=[f'sweep/{lang}-{size}.txt' for lang in ['rust','json','yaml','python']]
 cpu=statistics.mean(old[k]['syntect']['median_ms'] for k in ks)
 before=statistics.mean(old[k]['gpu']['median_ms'] for k in ks)
 after=statistics.mean(n[0][k]['gpu']['median_ms'] for k in ks)
 browser=statistics.mean(w[0][k]['median_ms'] for k in ks)
 lines.append(f'| {size} | {cpu:.3f} | {before:.3f} | {after:.3f} | {browser:.3f} | {browser/after:.2f}× |')
lines+=['', '## 两轮与尾延迟', '', '原生每轮每档 100 次；浏览器主测 100 次、复测 30 次，均在独立进程。下列数字均为 ms。', '', '| 样本 | 优化版 median / p95 | 原版 median / p95 | 优化版复测 median | 原版复测 median |','|---|---:|---:|---:|---:|']
for k in keys:
 a=n[0][k]['gpu'];b=w[0][k]
 lines.append(f"| {k.removeprefix('sweep/')} | {a['median_ms']:.3f} / {a['p95_ms']:.3f} | {b['median_ms']:.3f} / {b['p95_ms']:.3f} | {n[1][k]['gpu']['median_ms']:.3f} | {w[1][k]['median_ms']:.3f} |")
lines+=['', '## 验证', '', f"- {verification['cases']} 个案例、{verification['tokens']:,} 个 token 与原版比较，分类标签差异 **{verification['mismatches']}**。包含大代码、8B–8KiB 前缀、32-token 分块及树容量边界、中文/emoji、空输入、随机混合代码、输入变大/变小后的内存复用。", '- 最终 release build、Clippy（-D warnings）、rustfmt 通过。', '- 最终两轮原生和两轮浏览器均未检测到其他编译/渲染基准竞争。统计在实际读回并合并输出后结束；不含渲染、结果缓存命中和浏览器 IPC。', '- 样本为固定前缀，可不完整；不能把交叉点直接当作生产阈值。硬件是同一台 Apple M4 / 10 核 GPU，macOS 26.5，Chrome '+web[0]['browser']+'。', '- 原版只在生成 Metal 资产和验证/对比时使用；优化后的运行时不启动浏览器。', '', '## 初始化', '', '新进程初始化记录如下。系统 shader cache 已存在，不能代表第一次安装后编译成本。', '', '| 轮次 | 原生引擎初始化 ms | 原生首个输入 ms | 浏览器首个 parse ms |','|---|---:|---:|---:|']
for i in range(2):
 lines.append(f"| {i+1} | {native[i]['gpu_init_ms']:.2f} | {native[i]['results'][0]['gpu_first_ms']:.2f} | {web[i]['results'][0]['first_ms']:.2f} |")
lines+=['', '## 复现', '', '先按 README.md 加载构建环境及独立 CARGO_TARGET_DIR，然后：', '', '```sh', 'cargo build --locked --release --manifest-path benchmarks/gpu-lexer/Cargo.toml', 'LEXER_BACKEND=optimized uv run --script benchmarks/gpu-lexer/verify-optimized.py', 'python3 benchmarks/gpu-lexer/final-optimized-benchmark.py', 'python3 benchmarks/gpu-lexer/report-optimization.py', '```', '', '源文件：`src/metal.rs`；固定 Metal 内核与校验和：`assets/dawn/`；最终数值对照：`results/optimization/verification.json`；最终计时：`results/optimization/final-{1,2}/run-2.json` 和 `results/final-optimized-browser-{1,2}.json`。', '', '## 范围', '', '这是 macOS/Metal 的独立实验，尚未接入生产 GUI。没有验证其它平台、真实界面帧率、功耗、峰值内存或实际代码块分布。即使 GPU 更快，短块及缓存命中的取舍仍应结合实际场景。']
(root/'OPTIMIZATION-REPORT.md').write_text('\n'.join(lines)+'\n')
print(dict(primary_wins=wins,robust_wins=robust,total=len(keys),verification_tokens=verification['tokens']))

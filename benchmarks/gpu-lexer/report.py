"""Render recorded measurements; never benchmarks during report generation."""
from pathlib import Path
import json
root=Path(__file__).resolve().parent
runs=[json.loads((root/f'results/run-{i}.json').read_text()) for i in [1,2]]
assert all(json.loads((root/f'results/overlap-{i}.json').read_text())==[] for i in [1,2]),'Reject overlapping benchmark runs'
parity=json.loads((root/'results/parity.json').read_text())
lines=['# Rust + GPU 高亮实测', '', '后续已测原版 JS/WebGPU，性能显著好于这份 Rust 移植。请同时阅读 [原版浏览器报告](BROWSER-REPORT.md)；下文关于 JSON 的结论仅限本移植实现。', '', '2026-09-09，Apple M4 / 10 核 GPU，macOS 26.5，Rust release，wgpu 29.0.4 Metal / shader-f16。', '', '基线是 zork 当前 syntect 5.3.0 + regex-fancy + InspiredGitHub；测量重算高亮的完整 CPU/GPU 往返耗时，未计入生产结果缓存及界面渲染。主测量每个样本每个引擎 30 次，复测每个 10 次，交替顺序，两个独立进程。两轮均未检测到编译或 render-benchmark 竞争。另有两轮检测到编译竞争，已剔除，保留为 discarded 文件。', '', '## 预热结果', '', '以下列出主测量（30 次）median / p95（ms）。倍数是 syntect median ÷ GPU median；大于 1 表示 GPU 更快。第二轮（10 次）用于复测趋势检查，原始数据完整保留。', '', '| 输入 | 字节数 | GPU median / p95 | syntect median / p95 | 倍数 | 当前 zork |','|---|---:|---:|---:|---:|---|']
for row in runs[0]['results']:
    g=row['gpu'];s=row['syntect']
    st=f"{s['median_ms']:.2f} / {s['p95_ms']:.2f}" if s else '不支持'
    ratio=f"{s['median_ms']/g['median_ms']:.2f}×" if s else '—'
    lines.append(f"| {row['file']} | {row['bytes']} | {g['median_ms']:.2f} / {g['p95_ms']:.2f} | {st} | {ratio} | {'跳过高亮' if row['zork_would_skip'] else '执行高亮'} |")
lines += ['', '## 初始化和首次输入', '', '以下是新进程初始化；系统 shader cache 没有清空，不能视作首次安装后冷编译。首次实际创建 Metal shader 的开发运行曾观测到约 486ms，但当时有并行编译，不作为受控性能数据。', '', '| 轮次 | GPU 初始化 ms | syntect 初始化 ms | 首个 Rust 代码块 GPU ms | 首个 Rust 代码块 syntect ms |', '|---|---:|---:|---:|---:|']
for i,run in enumerate(runs,1):
    r=run['results'][0]
    lines.append(f"| {i} | {run['gpu_init_ms']:.2f} | {run['syntect_init_ms']:.2f} | {r['gpu_first_ms']:.2f} | {r['syntect_first_ms']:.2f} |")
lines+=['', '## 判断', '', '在保存的样本上，较大 Rust / Python / YAML 从 GPU 获益，JSON 则 syntect 更快；约 0.5KiB 的短片段全部是 syntect 更快。收益依赖语言与内容，不适合整体替换。16–60KiB 的 Rust/Python/YAML 是后续接入实验的候选范围，阈值仍需更丰富的真实代码样本确定。', '', 'GPU 仍有毫秒级固定开销，60KiB 的完整调用也超过常见一帧预算。若接入，应后台异步准备高亮并沿用结果缓存；这里没有验证界面帧率或功耗。', '', '## 移植正确性', '',f"原版 gpu-lexer 0.0.2 JS/WebGPU 与 Rust/Metal 对比：{len(parity['results'])} 个样本、{sum(r['tokens'] for r in parity['results']):,} 个 token，特征编码和 UTF-16 区间完全一致；分类标签差异 {sum(r['label_mismatches'] for r in parity['results'])}。UTF-8 高亮区间额外检查中文、emoji、CRLF。Chrome {parity['browser']}。", '', '这验证的是忠实移植，不证明模型高亮准确率等同 syntect。原模型仍是九类概率分类。', '', '## 两轮复测', '', '| 输入 | 第一轮 GPU / syntect median ms | 第二轮 GPU / syntect median ms |', '|---|---:|---:|']
for a,b in zip(runs[0]['results'],runs[1]['results']):
    def pair(r):return f"{r['gpu']['median_ms']:.2f} / "+(f"{r['syntect']['median_ms']:.2f}" if r['syntect'] else '不支持')
    lines.append(f"| {a['file']} | {pair(a)} | {pair(b)} |")
lines+=['', '## 交付边界', '', '- 实验位于独立 Cargo workspace，没有替换生产高亮，也没有改根 Cargo.toml/Cargo.lock。', '- Rust tokenizer、权重解码、七阶段 WGSL dispatch、缓冲区复用、同步回读与 span 合并已实现。GPU 模型与上游一致，没有重新训练。', '- 大于 64KiB 的结果是扩展能力对比；当前 zork 会跳过这些输入，不能视作现有应用提速。TypeScript 的 syntect 支持检查失败，未用其它语言冒充基线。', '- 未测试功耗、峰值内存、GPUI 帧率、其他 GPU / 操作系统以及首次安装后的 shader cache。性能结论只覆盖此 M4 和保存的样本。', '- 缓存命中应继续直接复用已有高亮；本实验不覆盖流式输入。', '', '完整方法和复现命令见 [README.md](README.md)。原始记录见 [results](results/)。']
(root/'REPORT.md').write_text('\n'.join(lines)+'\n')

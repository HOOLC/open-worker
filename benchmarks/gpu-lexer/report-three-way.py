"""Decision-oriented three-engine comparison of the exact-size sweep."""
from pathlib import Path
import json
import statistics
root=Path(__file__).resolve().parent
results=root/'results'
native=json.loads((results/'sweep-native/run-2.json').read_text())
browsers=[json.loads((results/f'sweep-browser-{i}.json').read_text()) for i in [1,2]]
assert not json.loads((results/'sweep-native/overlap-2.json').read_text())
assert all(not b['overlaps'] for b in browsers)
n={row['file']:row for row in native['results']}
w=[{row['file']:row for row in b['results']} for b in browsers]
names=['rust','json','yaml','python']
sizes=[2**i for i in range(3,14)]
lines=['# 三种高亮的性能比较与取舍', '', '**性能证据支持继续优化 Rust GPU 移植，当前生产高亮先保留 syntect。** 原版 WebGPU 在同一模型、同一 M4 上明显快于当前 Rust GPU；不能用当前移植的速度否定 ML 高亮。是否能在原生端追回差距仍需定位和实测，不保证一定能达到浏览器水平。', '', '## 8B → 8KiB 三方比较', '', '单位 ms，越低越好。每格是 Rust、JSON、YAML、Python 四种语言样本各自中位数的等权算术平均，不是实际聊天内容的流量加权平均。TypeScript 因 syntect 不支持而单列。此表不能代替下面各语言结果来设定统一阈值。', '', '| 输入字节 | syntect | Rust GPU 移植 | 原版 WebGPU |', '|---:|---:|---:|---:|']
for size in sizes:
 keys=[f'sweep/{name}-{size}.txt' for name in names]
 values=[statistics.mean(n[k]['syntect']['median_ms'] for k in keys),statistics.mean(n[k]['gpu']['median_ms'] for k in keys),statistics.mean(w[0][k]['median_ms'] for k in keys)]
 lines.append(f'| {size} | '+' | '.join(f'{v:.3f}' for v in values)+' |')
lines+=['', '## 怎么选', '', '1. 当前 Rust GPU 不适合全量替换：四语言平均值在 2KiB 及以下均慢于 syntect，4KiB 才开始胜出；JSON 到 8KiB 仍是 syntect 更快。', '2. ML 高亮值得继续验证：原版 WebGPU 的四语言平均值从 256B 档开始领先 syntect，8KiB 约快 8.7 倍；原版相对当前 Rust GPU 快约 4.7 倍（8KiB），小输入上差距更大。不同语言、前缀内容会改变交叉点。', '3. 原生优化应先定位固定开销：当前 Rust GPU 在多数小输入档位约 4–5ms，原版约 0.4–0.9ms。应分开测 CPU 分词/编码、GPU 内核、提交与同步回读，并比较 wgpu/Metal 与 Chrome/Dawn 的执行路径。现有 wall-time 不能判定具体慢在哪一项，也不能说明 Rust 语言本身较慢。', '4. 若原生能接近原版，可考虑按语言/大小路由，继续用 syntect 处理适合它的短块；若原生优化后仍有毫秒级额外开销而目标负载以短块为主，则保留 syntect 更合理。', '5. 本轮只回答性能取舍。模型标签与原版相同，不代表语法准确率等同 syntect；大规模替换还需要高亮质量验收。', '', '## 各语言完整结果', '', '每格为 median / p95（ms），WebGPU 复测列只给中位数。']
for name in names+['typescript']:
 lines+=['',f'### {name}', '', '| 字节 | syntect | Rust GPU | 原版 WebGPU | 原版复测 |','|---:|---:|---:|---:|---:|']
 for size in sizes:
  key=f'sweep/{name}-{size}.txt';native_row=n[key];br=w[0][key]
  def stat(s):return f"{s['median_ms']:.3f} / {s['p95_ms']:.3f}" if s else '不支持'
  lines.append(f"| {size} | {stat(native_row['syntect'])} | {stat(native_row['gpu'])} | {stat(br)} | {w[1][key]['median_ms']:.3f} |")
lines+=['', '## 测量边界', '', '- 同一台 M4 / 10 核 GPU，macOS 26.5；Rust release + wgpu/Metal；原版 Chrome 149 WebGPU hardware adapter，shader-f16。固定 gpu-lexer 0.0.2。', '- 相同 55 个精确字节数的代码前缀，可能不完整；较短前缀可能仅为注释或声明。这不是对实际聊天流量的抽样。', '- syntect 与 Rust GPU 每档各 100 次、同一进程交替计时；原版主测每档 100 次、独立浏览器复测 30 次。原生与浏览器串行运行，未做三个引擎同进程交替测试。', '- 有效数据中均未检测到编译/性能测试竞争。表格包含 CPU/GPU 往返和 span 合并；不含结果缓存命中、渲染及 IPC。原版返回 UTF-16 区间，Rust 另有 UTF-8 区间转换。', '- 未测功耗、峰值内存、真实界面帧率或实际代码块长度分布；不能据此声称整体应用提速。', '', '复现命令及样本见 [SWEEP-REPORT.md](SWEEP-REPORT.md)。此报告直接使用已完成的有效测量，不另跑重复实验。生成命令：`python3 benchmarks/gpu-lexer/report-three-way.py`。']
(root/'THREE-WAY-REPORT.md').write_text('\n'.join(lines)+'\n')

# 客户端订阅验证记录

2026-09-11。对应[订阅合同](client-state-subscriptions.md)与[设计记录](client-subscription-design.md)。本记录区分核心、wire 参考镜像、编译与可见呈现；不把微基准换算为设备 FPS。

## 核心与 wire

在同一台开发机、相同 release 配置下，分别加载 1,000 / 10,000 / 100,000 条、12 种混合消息，使用 1 / 8 个消费者，每个场景 100 次采样。先构建，再单独运行二进制，测量期间没有其他 Cargo/rustc 构建。原生基线来自改造前的真实 Conversation reducer/subscription；不是存储分页或空 channel 基准。

十万条消息、8 个消费者，核心提交与全部消费者读取的合计耗时：

| 操作 | 改造前 p95 | 改造后 p95 | 改造后 p99 |
| --- | ---: | ---: | ---: |
| 追加一条 | 38,472.58 µs | 5.33 µs | 8.13 µs |
| 替换尾部正文 | 42,713.38 µs | 3.00 µs | 3.58 µs |
| 重复相同消息 | 14,376.79 µs | 0.79 µs | 0.83 µs |
| 仅 activity | 0.46 µs | 1.17 µs | 1.83 µs |

activity 原本不遍历消息，这次增加了版本与确认开销；该项没有性能提升。消息路径消除了整列表深复制、逐消费者扫描和全量字节重算。单条数据的共享测试还验证：保留十万条旧快照，再追加、前插及修改一条，只复制被修改的那条 payload。

独立 wire 场景同样有十万条核心历史，请求范围为最近 100 条，追加内容轮流覆盖混合长正文。一次追加的核心提交、DTO 准备、JSON 编解码及参考镜像应用合计 p95 为 **26.33 µs（1 消费者）/ 176.96 µs（8 消费者）**。p95 编码量分别为 11,477 / 91,816 字节；追加只编码一条记录及必要的窗口删除。1,000 与 100,000 条历史的单消费者追加编码量分别是 11,465 和 11,477 字节，差异来自 ID/计数位数。

重复输入在 wire 场景中为 **0 次唤醒、0 字节**。这些数值不包含 JNI envelope/UTF-16 转换、Java 对象、Kotlin 调度或真实屏幕呈现，参考镜像也不是 Compose 性能测量。小样本绝对耗时受进程预热和主机调度影响，完整分布保留在原始报告中。

分配计数使用另一个带 System allocator 计数器的可执行文件，避免给前后耗时比较引入测量开销。十万条、8 消费者追加的 p95 为 110 次分配调用（含 realloc）、15,783 字节分配请求、9,217 字节额外存活峰值；1,000 条时分配请求为 20,054 字节，没有随历史正文量线性增长。10,000 次未消费突发后，核心日志保持 512 条，估算保留 144,896 字节。业务历史本身新增的记录与日志保留分开统计，不能将其解释为进程内存硬上限。

原始记录（本地验收产物）：

- 改造前（本地生成的验收记录）
- 最终核心与 wire 分布（本地生成的验收记录）
- 分配与慢消费者日志（本地生成的验收记录）

## 行为与平台检查

已执行的相关检查：

- `zork-observe`：5 个集合/值单测、10 个协议回归；包含首次空值、无 PartialEq 类型、取消、读后并发写、ACK 竞争、topic 路由、10,000 次突发、条数/字节超限、关闭、源替换与紧急撤权。
- `zork-client-core`：77 个单测和相关 API、客户端边界、设置副本提交、重连/投递等集成回归通过；`zork-client-types` 的 21 个单测通过。设置回归还覆盖仅推进存储游标不唤醒 UI，以及任务延后启动仍能发布已过期的在线状态且不重复唤醒。需要独立 Station 的 enrollment 用例按其配置忽略，本轮未执行该环境测试。
- 新 wire 回归以十万条权威数据验证：只重置请求的 100 条；取消准备后积压 600 次提交仍可与快消费者收敛；扩大范围只影响自己的订阅；删除 outbox 回执不冒充送达。固定锚点在 1,000 条突发后保持原阅读范围，并能向前、向后加载或回到尾部。
- History 的 10,000 条回归验证两个远距离变更只产生两个 entry edits、未改记录继续共享、取消后可重放，撤权使在途旧批次失效。
- Android host 回归覆盖：持有命令锁时独立回调/读取仍工作，2,000 次草稿更新只产生一次待处理通知，关闭释放观察，A → B → A 的旧 generation/句柄不能读取或确认新订阅。JNI arm64 交叉编译、Kotlin 主程序和 instrumentation 源码编译通过；JNI 导出及 Kotlin `onReady(long,long,boolean,boolean)` 描述符匹配。
- `python3 scripts/check-client-boundary.py` 与 diff 空白检查通过。
- GPUI `headless_isolation`、`headless_history`、`headless_presence` 通过。1,000 次 core 提交在下一帧前不改变呈现镜像，下一帧完整补齐且只渲染一次 transcript。历史分别检查 1280×800 与 900×600 的布局、折叠、时间轴、实时来源和滚轮。
- GPUI Web 通过公开 core 依赖构建，使用当前锁定的 WASM 工具链与内存 fixtures。该项验证编译边界，不代表完整 Web 会话桥接或浏览器帧率验收。

成员弹层检查曾在反向采样之间保存 PNG，真实时钟开销会影响这段短动画的断言。将截图 IO 移到反向位置断言之后，保留原位移约束后通过；没有修改生产动画曲线。渲染压力回放还发现离线 fixture 的重发标记更新后缺少 transcript 失效通知，已补齐 fixture 路径，保留原投递状态验收负载。

## 十万条原生渲染

使用原有全类型 fixture、正文 13 px、起始/中间/末尾位置及两次确定性回放。记录 CPU draw 耗时、实际数量、类型覆盖、解析/可见行数，以及文件预览和 pending/sending/failed 交互。最终数据见本地 渲染报告目录（本地生成的验收记录）。

最终回放通过，三个位置均为实际 100,000 条，31 种正文类型均被采样，另有 6,451 个文件/图片条目。每个位置 240 个虚拟帧，两次回放的图像与交互检查通过。

| 位置 | 改造前 p95 / p99 CPU draw | 改造后 p95 / p99 CPU draw | 最大构建消息行 |
| --- | ---: | ---: | ---: |
| 0 | 0.701 / 0.749 ms | 0.695 / 0.741 ms | 10 |
| 50,000 | 0.789 / 1.092 ms | 0.779 / 1.122 ms | 11 |
| 99,960 | 0.793 / 0.977 ms | 0.788 / 0.876 ms | 11 |

滚动成本与基线接近；中间位置 p99 略高，所有原有门禁均通过。此处是离屏 CPU 绘制与确定性回放，`virtual_fps` 是测试时钟输入，不能报告为显示器实际 FPS。

三个位置解析过的文档分别为 75 / 125 / 125，展开文件列表每帧最多构建 9 行。包含字体、图片、布局和先前回放缓存的累计进程峰值 RSS，从 778.55 / 816.56 / 853.75 MiB 变为 786.02 / 824.31 / 863.52 MiB；该项略增，与上面的订阅分配和日志保留指标分别记录。

## 重现与边界

```sh
python3 scripts/lib/build_env.py -- env CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 cargo bench --locked -p zork-client-core --features headless-bench --bench subscriptions --bench subscriptions_alloc --no-run
python3 scripts/lib/build_env.py -- env CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 cargo build --locked -p zork-gui --features headless-bench --bin zork-gui-render-bench
```

构建完成且无其他编译/负载测试时，单独执行 Cargo 打印的两个 benchmark 路径，stdout 保存为对应 JSON。渲染使用配置中的 target 下 `debug/zork-gui-render-bench`，设置 `ZORK_SCROLL_ALL_MESSAGES=1 ZORK_BENCH_MESSAGE_COUNT=100000`，传入独立输出目录。不要通过降低消息数量或修改门禁取得通过。

本轮没有安装到个人设备，也没有执行真实 Android/原生显示器 FPS 测量。首轮保留的 History 全量业务重放已在[后续增量聚合改造](history-incremental-validation.md)中处理；本页早期测量不包含该后续改动。小目录的比较与同步消费、尚未接入的流式文本块和完整 Web 会话端，仍保留在当前合同的迁移边界中。

已删除本任务生成的 Web staging（154 MiB），保留构建摘要（本地生成的验收记录）、基准原始数据、截图与小型源码基线备份；未清理共享编译缓存或其他任务产物。

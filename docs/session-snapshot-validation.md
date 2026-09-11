# Session snapshot 协议与验证

2026-09-11。对应[当前订阅合同](client-state-subscriptions.md)及[Session 历史](session-history.md)。

## 当前行为

- 每次 Session SSE 连接都先返回当前 snapshot，再发送该边界之后的事件。Last-Event-ID 不取消首帧 snapshot；广播落后重新取得 snapshot，不补放整段历史。
- 累计 usage、运行次数及两条最近完成活动随执行节点原有 SessionState 快照保存，并在事件提交时增量维护。概览读取不克隆 generation transcript；空闲时从最新 snapshot 与 append-only 尾部恢复，不走全档案 fallback。
- 老快照缺少聚合字段时标明覆盖不完整，UI 显示未知。上下文切换保留累计统计，当前上下文输入数重新等待 provider 实报。
- 客户端只在 Rust core RAM 保存概览和用户按需加载的具体执行记录。历史端点不返回 snapshot/runtime；历史分页不能改变累计统计。已送达聊天消息的磁盘缓存遵循其独立契约。
- GPUI 统计和 hover 共享 Conversation overview topic；普通聊天呈现与 Android 消息 wire 不订阅这个 topic。Station 任务计数从 snapshot 建立，游标单独推进不产生业务唤醒。

## 已执行的行为检查

- Agent HTTP：首次连接、Last-Event-ID 重连都首先收到 snapshot；只发送后续事件。容量为 1 的强制广播落后恢复后状态正确，history scan 次数不增加。
- Agent 状态：累计 usage、运行次数和最近活动跨快照保存、服务重启后保留，并接续新提交；旧快照覆盖为未知；缺少恢复起点时禁止全档案/历史 fallback。
- Client core：snapshot 在聊天消息 catch-up 被阻塞时已可读取；打开概览没有 history 请求，显式加载详情才读取。详情中的 usage 不覆盖 snapshot 累计值；概览更新不刷新详情。
- 订阅语义：仅游标变动不发布概览，错误 session 的更新被拒绝；撤权清除概览且迟到 snapshot 不能恢复内容。
- Station：只有最近一条运行记录时仍从 snapshot 得到 100 次累计运行；重复/仅游标推进不触发同步版本或业务唤醒，新运行和重启后的计数仍正确。

最终回归结果：

| 检查 | 结果 |
| --- | --- |
| Agent HTTP / snapshot / context / supervisor / recovery invariants | 34 项通过 |
| Client core 单测与 shared runtime | 84 + 8 项通过 |
| Station | 93 项通过，含并发首快照及任务计数/静默游标回归 |
| `headless_history` | 1280×800、900×600 通过 |
| `headless_history_statistics` | 两种尺寸通过；真实分页更新不改变累计统计，拖拽/滚动保持正确 |
| `headless_presence` | 动效、生命周期、几何及空闲重绘检查通过 |
| core/UI 边界、扫描器自测、skill 结构、修改文件格式和空白 | 通过 |

统计页使用 600 条记录、120 帧滚动，在没有其他 Cargo/rustc 构建时单独运行：

| 窗口 | 绘制 p95 | 绘制 p99 | 最大构建行数 |
| --- | ---: | ---: | ---: |
| 1280×800 | 4.297 ms | 4.395 ms | 16 |
| 900×600 | 3.969 ms | 4.030 ms | 12 |

两者均通过既有 8.33 ms p95 门禁。这是本轮 headless 绘制耗时，不从中推导实际屏幕 FPS，也不把既有详情 ledger 微基准当作新 SSE 协议的网络性能数据。

首次 Station 回归发现游标记账触发额外 WORK 唤醒，修复后完整复测通过。统计页原拖拽 fixture 没有保留抓取偏移，已改为按实际分隔条位置往返，保留原宽度断言。并行新增代码的两处编译引用/错误转换问题做了最小适配。链接一度因磁盘不足失败；确认进程已退出后，只清理本任务的过期测试二进制，并成功重新构建。

逻辑检查命令与结果（本地生成的验收记录）、桌面检查结果（本地生成的验收记录）、源码与产物摘要（本地生成的验收记录）、1280 绘制报告（本地生成的验收记录）、900 绘制报告（本地生成的验收记录）、统计页截图（本地生成的验收记录）。

本轮未创建测试应用包、安装设备或独立 target；当前测试二进制使用配置中的共享构建缓存。保留小型源码基线、测试日志和截图，用户数据及其他任务产物未清理。

## 复现

所有 Cargo 命令先经 `scripts/lib/build_env.py -- env CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4` 加载当前构建环境：

```sh
cargo test --locked -p zork-agent-testkit --test http_api --test session_snapshot --test context --test supervisor_contract --test recovery_invariants
cargo test --locked -p zork-client-core --lib --test shared_runtime
cargo test --locked -p zork-station --bin zork-station
cargo test --locked -p zork-gui --features headless-bench --test headless_history --test headless_history_statistics --test headless_presence --no-run
python3 scripts/check-client-boundary.py
```

编译结束、没有其他编译负载时，分别执行 Cargo 输出的三个 headless 测试二进制。统计页可以用 `ZORK_HISTORY_STATISTICS_OUTPUT` 指定截图与绘制耗时报告目录。这里验证的是受控 provider、实际 HTTP、core 状态和 headless GPUI；不代表真实远程模型、多设备部署或显示器 FPS。既有[十万条详情匹配基准](history-incremental-validation.md)测的是用户已请求数据的物化和更新，不是打开概览的成本。

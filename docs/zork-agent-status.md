# zork-agent 实现与验收状态

更新于 2026-09-11；下列结果记录各自测试时的源码与环境，不代表后续工作区已通过验证。目标设计以
[`zork-agent-architecture.md`](./zork-agent-architecture.md) 为准；本文只记录实现状态和证据。

## 2026-09-11 Session snapshot 与按需历史

- Session SSE 每次连接与重连先发送当前 snapshot，再传后续事件；广播落后重建 snapshot 基线，不回放历史。
- 累计用量、运行次数、最近活动随既有状态快照保存并增量更新。统计和预览使用 core 内存概览，显式历史分页不再携带 runtime/snapshot。
- Agent 相关 34 项、core 92 项、Gateway 93 项测试通过；桌面历史、统计与预览回归通过。范围、性能口径与复现命令见[验证记录](session-snapshot-validation.md)。

## 2026-09-10 DeepSeek 运行通知续接

- 投影为运行时合成调用设置明确来源标记；DeepSeek Responses 在思考模式下仅为这些调用附加固定非空 reasoning 说明。真实 provider output 原样保存与回传，其他供应商和关闭思考不添加说明。
- `reasoning_text` 缺失的确定性 HTTP 400 不再重试十次；上下文溢出、限流和服务端错误继续使用原有分类。
- 97 项相关 Rust 回归通过，涵盖来源隔离、流式/非流式等待通知、重复通知、磁盘重放、迟到结果、交接、Codex 续接及错误重试；修改文件的格式与 diff 空白检查通过。
- 重建独立 Agent 后，真实 DeepSeek `deepseek-flash` / high / streaming 验证了等待到期后的工具执行，以及等待期间重启后的恢复执行；两轮均完成，各产生一次等待到期事件，无 provider 失败。
- 脱敏验收记录见 `artifacts/deepseek-runtime-notice-20260910/verification.json`（本地生成的验收记录）。隔离测试进程、临时二进制和凭据/会话目录已清理；本次不代表其他供应商真实 API 已验收。

## 2026-09-09 通知、工具边界与故障恢复

- 运行时通知使用稳定的合成 call/result 对；真实用户输入保留 User role。按内部 invocation ID 配对当前调用的结果，重复 provider call ID 和迟到 end 结果不再改写已发送前缀。
- 等待结束不冻结结果：下一次模型请求冻结前允许合并；已发送未完成占位后的结果作为新通知。tool.help 的知识版本只在结果实际进入请求时更新；end 必须先披露 outstanding，才能接受对应的 acknowledge。
- tool.cancel 使用独立有界取消名额，通过普通工具执行器进行版本与参数处理，由会话单写者持久化取消请求后发出信号。逻辑工具拥有参数解析，通用 schema 层不再静默删字段。标准 Responses 的完整但畸形调用参数进入工具错误反馈。
- runner 退出会终止并等待模型任务；失败/取消状态在列表保留。Codex 请求取消时丢弃连接及续传状态；空闲缓存有 60 秒 TTL、32 条连接和 64 MiB 输入估算预算，15 秒周期回收，排除活动请求。
- 本轮受影响 Agent 包 165 项 Rust 测试通过；真实 WebSocket 对端覆盖取消后的新连接。重建四个后端二进制后，embedded Gateway 进程回归通过，覆盖真实 shell 清理、重启恢复、不重复执行与目录排他。
- 证据在 `artifacts/agent-fixes-20260909/`。未运行真实模型长任务或独立性能基准；缓存预算测试不代表进程 RSS 实测。原设计中的第三方 WASM 工具执行仍是待实现能力，不纳入本轮缺陷修复完成声明。

## 2026-09-07 Gateway 嵌入

- Gateway 持有 `AgentRuntime`，所有内部 Agent 操作和状态观察直接调用库。Agent HTTP/SSE 作为同一进程的外部兼容接口保留，PID 与 Gateway 相同；supervisor 只管理一个 Gateway 子进程。
- readiness 覆盖 Agent 初始化；关闭时先排空 Agent 和状态订阅，再关闭 HTTP 与 Synch。恢复中的工具回调可以等待已绑定的监听器启动。
- 首次从双进程版本迁移需完整重启 supervisor 或执行原生版本升级；新 `zork update` 检查运行中 supervisor 的 `agent_mode`，不向旧 supervisor 发送 reload。后续 update / reload-mesh 都重启 Gateway 与 Agent。
- 179 项相关 Rust 测试通过，15 项 Gateway / mailbox / merged runtime / supervisor / admin JS 测试通过；改动 Rust 格式、JS 格式/lint 与 diff 空白检查通过。
- 真实进程验证通过：Gateway 与 Agent PID 一致、无 sidecar、目录重复打开被拒绝、SIGTERM/SIGINT 清理真实 shell、会话重启恢复且不重跑工具。
- 真实桌面入口、4 项 supervisor 原生切换合同、完整认证 HTTP 原生升级通过。真实 mesh 的跨节点执行/文件交付、执行中 Gateway 崩溃恢复、去重、离线恢复、取消和 owner-only CAS 接受均通过。
- 验证使用 mini1 的隔离 data root、假模型或本地受控 provider，没有切换用户运行中的部署。

复现本轮验证：

```sh
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=4
cargo build --locked -p zork -p zork-gateway -p zork-gh -p zork-agent-server
cargo test --locked -p zork -p zork-gateway -p zork-agent -p zork-agent-http -p zork-agent-server -p zork-agent-gateway-tools -p zork-agent-testkit
npx --yes pnpm@10.33.0 exec vp test --run test/gateway-mailbox.e2e.test.ts test/merged-runtime.e2e.test.ts test/zork-update.e2e.test.ts test/rust-runtime.e2e.test.ts test/control.e2e.test.ts
python3 crates/zork-gui/tests/test_gateway_entry.py
python3 scripts/test-embedded-gateway.py
python3 scripts/test-native-upgrade.py
python3 scripts/test-gateway-upgrade.py
python3 scripts/test-mesh.py
```

## 2026-09-07 嵌入式库边界（第一阶段）

- `zork-agent` 只提供核心库；HTTP/SSE 适配、独立进程宿主和 gateway 专属工具分别移到 `zork-agent-http`、`zork-agent-server`、`zork-agent-gateway-tools`。
- 新增 `Agent` 共享应用接口和 `AgentRuntime` 组装/关闭接口。业务校验及事件历史重放、去重、落后补读由核心提供；profile 刷新、deadline 调度和消费任务在关闭时终止并等待结束。
- 核心包普通依赖树不包含 Axum、HTTP 适配或 server 包。库不监听端口，也不初始化全局日志或信号。
- Agent / HTTP / server / gateway-tools / testkit 共 141 项 Rust 测试通过，包含新增无 HTTP 的执行与重启、事件补读和 scheduler 关闭回归；真实 Gateway 桌面入口的 1 项 Python 合同通过。改动文件 rustfmt 与 diff 空白检查通过。
- 二进制仍叫 `zork-agent`，构建选择改为 `-p zork-agent-server`；发布、Docker 和桌面构建脚本已更新。这一阶段保留独立 Agent 进程，后续 Gateway 嵌入见上节。

复现本轮验证：

```sh
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=4
cargo test --locked --offline -p zork-agent -p zork-agent-http -p zork-agent-server -p zork-agent-gateway-tools -p zork-agent-testkit
python3 crates/zork-gui/tests/test_gateway_entry.py
```

## 2026-09-05 已完成

- 持久上下文策略：默认 compaction，支持 handoff；策略变化只影响下一次上下文整理，恢复保留进行中的计划。
- Agent HTTP、Gateway、Admin 和桌面端共用 Agent 持久化的上下文配置。Admin 支持自定义保留 token 数，桌面支持常用值及当前自定义值；Gateway 不保存策略副本。
- 工具结果摘录先保留调用参数、实际 offset、退出码等元数据，再截取大正文；摘要指令区分事实、检查结果与计划，并要求纠正旧摘要中的错误。file.read/file.edit 的初始说明包含正确参数。
- Gateway 测试替身和断言已跟随新增 context 字段更新；target-linux 已从 Git 和格式/lint 扫描中排除。
- 启动目录发现最多使用四个元数据读取线程，最终排序和恢复优先级保持不变。
- 向前查询先定位返回范围，再只解码需要返回的事件；解码借用原始 JSON。两次扫描复用同一文件句柄，避免日志轮转替换路径造成读取竞态。
- 所有独立 benchmark 入口接受 Cargo 自动传入的 --bench 参数；启动和查询压测失败也输出指标。

## 回归与实测

- Rust 全工作区 231 项通过（后端 200 项、原生 GUI 31 项）；Agent / testkit 其中 128 项通过，含 compaction、handoff、恢复、取消、迟到工具结果、HTTP/SSE、provider 对端和真实 shell/文件系统合同。
- JS / Admin UI：26 项行为与集成测试通过。DeepSWE adapter 的 66 项单元测试在独立、按路径触发的工作流运行。
- Admin TypeScript、构建、lint 和格式检查通过；会话列表的返回类型显式标注为 SessionRecord。
- 真实 Agent + Gateway 桌面入口测试通过，包括上下文读写、非法值拒绝和显式消息边界。
- 界面实测：Admin 保存 12345 tokens，切换 handoff 并刷新；桌面读取这个自定义值，改为 compaction/8000，Admin 刷新显示一致。

mini1 release 基准（fixture 构造不计入测量）：

| 场景 | 结果 | 门槛 |
| --- | --- | --- |
| 10000 个虚拟 session 完整生命周期 | 1.277 秒，7833 sessions/s，90000 events | 保留 release 基线 |
| 131072-event session，1024 次查询 × 1024 events | 串行 37/s，8 workers 192/s，峰值 RSS 13.5 MiB | ≥20/s，≤256 MiB |
| 32 MiB segment，zstd level 12 | 2.01×，24.6 MiB/s，峰值 RSS 50.8 MiB | ≤256 MiB |
| 100000 session 启动 | 总计 6.511 秒；发现 2.532 秒；后台 3.977 秒；峰值 RSS 101.4 MiB | ≤10 秒，≤512 MiB |
| 100 sessions × 100 fragments × 至少 16 MiB，10000 次随机查询/档 | 10 workers 670.5/s，p95 29.1 ms，p99 36.0 ms；峰值 RSS 238.1 MiB | ≤256 MiB |

启动精确恢复与请求优先恢复均低于毫秒输出精度，并分别只恢复一次。优化前同机总耗时为 10.982 秒，未通过 10 秒门槛。

极限查询压测优化前峰值为 357.7 MiB，超过 256 MiB 门槛；最终在相同默认负载、系统分配器下测得 238.1 MiB。为避免保留大量原文和反复解码废弃记录，before 查询先定位窗口，再从同一打开的文件句柄读取返回范围。代价是额外扫描：同机片段查询从串行 54/s、8 workers 270/s 降至 37/s、192/s，仍高于 20/s 门槛。

分配器替换和固定线程池实验没有提供足够收益，均已撤回；最终验收未连接堆采样工具。

测试精简后，本机后端 200 项测试的执行时间合计约 7.5 秒（不含编译），原生 GUI 31 项约 0.02 秒，JS / Admin UI 26 项约 8.9 秒。默认测试移除重复的 debug 性能跑分、源码字符串/样式常量自测和旧直连 Agent 的 GUI 假服务自测；性能门槛仍由独立 release benchmark 验收。普通 CI 覆盖全部后端和 Admin UI；原生 GUI 按相关路径独立运行。

本地原始日志、截图和跨端验证结果保存在 `artifacts/agent-readiness-20260905/`；本轮测试和 CI 日志在 `artifacts/merge-agent-readiness/`。

## 仍需实证的模型行为

本轮修复了可复现的摘要证据丢失和工具指引问题，但没有重新运行真实模型的 Wasmi 长任务。不能据此宣称已经消除重复探索或提高解题完成率。

下一次长任务验收应固定源码、模型与预算，以实际代码变更、完整检查输出、任务要求的提交和 verifier 结果为完成标准。比较压缩前后保留的硬性要求、工具参数纠正、重复读取比例及未缓存 token 消耗；不以 step/compaction 次数作为任务进展。

## 复现命令

在 mini1 仓库执行，沿用 AGENTS.md 的构建资源约束：

```sh
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4
cargo build --locked --workspace
cargo test --locked --workspace
npx --yes pnpm@10.33.0 test
npx --yes pnpm@10.33.0 format:check
npx --yes pnpm@10.33.0 lint
python3 crates/zork-gui/tests/test_gateway_entry.py
npx --yes pnpm@10.33.0 benchmark:deep-swe:test
cargo bench --locked -p zork-agent-testkit --bench test_world --bench startup_recovery --bench query_api --bench segment_compression
cargo bench --locked -p zork-agent-testkit --bench query_pressure
```

性能基准应独立运行，避免同时编译或执行其他负载测试。

### 2026-09-09 图片工具结果

`file.read` 支持 PNG/JPEG/GIF/WebP 完整图片结果，携带到标准 Responses、Codex、Anthropic 和 Chat Completions 适配器；服从逐模型 image 输入配置。图片持久化和重启恢复已覆盖，文字分页保持原行为。agent 与 testkit 共 167 个测试通过，包括真实 HTTP 请求验收；证据在 `artifacts/file-read-image-20260909/tests-final.log`。不把本地 HTTP fixture 验收当作真实厂商模型识图验收。

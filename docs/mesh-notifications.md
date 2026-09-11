# Agent Mesh 订阅与通知基建

`zork-notify` 提供业务共用的变化通知、事件分发和订阅生命周期；
`zork-mesh::feed` 把同一套订阅接到已认证的 Mesh stream。
Station 的 HTTP/SSE、Mesh 和进程内读取共用业务 source。
客户端继续由 `zork-client-core` 协调持久副本、合并状态并发布只读变化，UI 不管理业务连接或刷新时钟。

## 业务接入

一个业务只需要确定数据权威、变化主题、读取/分页方式和权限规则。

1. 创建或复用 `Hub<DomainTopic>`，按设备、会话或操作 ID 选择主题。
   `subscribe(topics)` 注册精确兴趣；多个主题的一次提交只唤醒同一订阅者一次。
   单个变化源可直接用 `Notifier`。
2. 在真实提交成功后 `publish(topics)` / `notify()`。只读请求、回滚、相同内容和
   内部回执记账不应产生业务变化。通知并不负责替业务执行事务。
3. 实现 `stream::Source`：`read` 校验当前权限并读取权威；有权限边界的 source 还实现 `check_access`，用于排队时只校验权限。返回 `Page::snapshot`
   或 `Page::chunk`。`more` 表示继续排空当前分页，`done` 表示本次读取结束。
4. 使用 `stream::spawn` 编码传输帧；`stream::merge` 合并独立来源，
   `stream::guard` 将权限变化与连接生命周期绑定。业务不再复制等待、定时刷新、
   慢消费者处理或连接关闭循环。
   临时能力通过 `Source::expiry` 提供绝对到期时间，读取、空闲和满队列时都执行到期终止。
5. 远端使用 `MeshNode::follow` 和只读 `feed::Watch` 请求类型，业务命令不能进入自动重连。消费者提交数据和持久游标后，调用
   `Feed::resume_with` 更新下次握手的读取位置；它拒绝更换订阅范围或回退游标。临时断线重连由 feed 处理。

```rust,ignore
let changes = topics.subscribe([Topic::Assignment(assignment_id.clone())]);
let stream = zork_notify::stream::spawn(
    AssignmentSource::new(database, authenticated_peer, committed_cursor),
    changes,
    16,
    Some(zork_mesh::feed::HEARTBEAT),
    encode_frame,
);
```

`AssignmentSource` 和 `encode_frame` 是业务适配器。实际接入可参考
`gateway::mesh::MeshWatch`、`gateway::desktop_events::CatalogSource` 和
`gateway::tool_stream::ToolSource`。新增 Mesh 主题加入 `zork-mesh::feed::Watch`（Station 的 `WatchTopic`）
允许列表，复用 `{"v":1,"request":{"kind":"watch","topic":...}}`；
主题名称不授予访问权限。既有 `watch_peer`、`watch_assignment` 和客户端
`subscribe` 请求继续适配到共用 source。

## 顺序、背压与恢复

- **先订阅，再读取。** `Changes::checkpoint` 在读取之前调用；读取期间发生的变化仍然待处理。
  `Changes::merge` 组合数据与权限来源，不增加转发任务。
- **通知可以合并。** 每个订阅保存一个待处理变化，不积累过期通知。
  快照内容相同不重复发送；日志分页继续排空，避免等待下一次变化才发现后续页。
- **游标有两种职责。** `Source::delivered` 只推进当前连接已入队的发送位置。
  接收端只有在数据落盘后才能推进其持久游标。流关闭和重连从接收端已提交的位置恢复。
  队列接受一帧不等于对端已经持久化，更不等于业务命令执行成功。
- **队列有界。** 背压期间不读取业务数据、不推进尚未发送页的游标；新通知只触发权限检查，有发送容量后才读取最新状态。
  `guard` 即使遇到满队列也能停止上游。订阅结束不能撤回已交付的数据。
  source 还需遵循相应协议的帧大小限制；Mesh 控制帧上限仍为 256 KiB。
- **瞬态事件显式报告丢失。** `events::EventHub` 为会话正文增量、活动等提供有界 fan-out；
  `Lagged` 必须转换为业务的重新同步提示，不能把丢失当作完成。Session 状态流用新的权威 snapshot 建立基线；
  历史明细另行显式分页读取，不为恢复状态聚合而重放全部历史。
- **订阅有所有者。** 释放订阅立即移除主题兴趣；释放 feed 或 `Task` 终止其 IO。
  合并来源轮转调度，高频状态不能挤住消息。任一来源关闭时，整条合并流关闭以触发重新订阅，避免仅部分数据仍在更新。
  关闭观察者不等于取消仍在执行的业务任务，取消继续走原有命令与回执协议。

本地通知 revision 与 Mesh 管理的 `change_token` 仅用于失效判定。
`change_token` 包含进程 epoch，消费者只判断相等与否；它不是可持久化的同步游标。
业务同步继续使用既有 owner / epoch / scope / sequence 合同，命令 ID 与重放规则不变。

### 频道通知与 Agent 输入

频道业务接入 `Watch::AgentMessages { epoch, after }`。订阅范围由已认证的接收节点确定，
请求中没有可任意替换的 agent_id；一条节点间连接复用该节点所有 Agent 的频道通知。
实现位于 `gateway::channels::delivery` 和 `gateway::db::chats`，没有第二套 Mesh 重连机制。

这里有三个独立确认：发送 source 的入队位置、接收节点已经与收件箱一起提交的持久游标，
以及 Agent 已持久接受的输入来源水位。只有第二步完成后才更新 Feed 的重连请求；
第三步完成后 Station 才写入投递完成回执。Agent 水位在快照中保留，因此回执丢失、
夹入其他来源的输入或重启不会造成相同输入再执行一次。

频道偏好另有代次。接收端丢弃已被较新设置撤销的晚到通知，并在投递 Agent 前再次检查。
已进入 Agent 的输入不撤回。on_next_turn 输入不唤醒空闲模型；即时输入和正常模型边界仍由 Agent 管理。
不同 Agent 的收件 worker 分别退避，单次交付和同时活动数量有界；同一个 Agent/来源不越过失败的较早位置。

这些水位都不是 UI 的 applied version。zork-observe 的 prepare/ack/discard 继续只负责消费者真正应用的状态基线。
完整业务合同见 [Chat 与 Agent 工具](chat-tools-design.md)。

## 已接入的链路

| 业务/来源 | 公共机制与行为 |
| --- | --- |
| Station SQLite 提交 | `Realtime` 的事务 hook 接入主题 Hub；保留旧 revision 接口供兼容调用者使用 |
| 配置、Profile、升级状态文件 | 内核文件通知；相同指纹不发布，目录替换通过父目录重新注册 |
| 客户端 catalog 与会话 | 同一 source 供 SSE 与 Mesh；快照和会话瞬态事件通过公共合并流输出 |
| Agent 会话事件、Station 本地消息 | 共享 `EventHub`，按会话隔离，无订阅者时不保留事件 |
| Mesh peer、成员目录、任务事件 | 共用 `Source` 与 `Feed`；恢复时读取成员快照或已提交任务游标 |
| Chat 频道与 Agent 收件 | 按接收节点复用 `AgentMessages`；消息与通知、收件与游标分别同事务提交，Agent 接受使用持久来源水位 |
| 邀请管理 | 管理节点的邀请变化即使不改变成员 revision，也通过管理 token 传到其他节点；core 观察共享 Device 更新 |
| device/MCP 工具输出 | 同一分页流驱动；输出偏移随交付推进，MCP 终态结果只序列化一次，逐页复查授权 |
| 工具、MCP 和服务隧道撤权 | 等待权限来源的变化；没有每秒查询权限的循环 |
| 共享服务进程 | 子进程退出/父进程管道 EOF 唤醒服务协调，无空闲存活检查循环 |
| 本机节点状态 | 只读 supervisor `observe` 连接通过 EOF 通知退出；文件变化通知设置和启动；观察连接不获得节点 lease |
| 设备同步与设置页 | 真实变化触发刷新；失败需求保留并退避重试，成功后停止；不再每 30/60 秒全量刷新 |
| Android / 桌面升级 | 同一个 `Device::upgrade` 提交一次，订阅共享设备状态；版本与 `operation_id` 隔离旧结果，停止观察不重发安装命令 |
| CLI 升级、本机启停与后台服务 | `service::Events` 合并精确文件和进程事件；只有已声明就绪却验证失败时才退避，健康状态不定期探测 |
| Agent 会话压缩 | 启动恢复扫描、段封存通知、失败退避；无候选时不再每 30 秒扫描 |
| 手机邀请审批 | `Watch::Invitation` 只观察已验证身份自己的 claim；批准、撤销、到期和成员移除终止或推进 core 状态机 |
| 浏览器命令 | 同一 source 提供 HTTP POST SSE / Mesh 订阅；回执独立提交，连接 generation 与持久授权撤销阻止旧连接恢复 |
| 浏览器标签页、连接和画面 | core 按 host 发布变化，GPUI 通过 `FrameDelivery` 合并，显示最新画面；取消 16ms 状态检查 |

文件 source 对所需目录使用非递归注册，不扫描节点内的仓库或全部会话目录。
父目录也被注册，以捕捉原子替换和尚未创建的 Profile 目录。
回调忽略读取事件，并处理后端要求重新扫描的通知；读取失败保留最后有效指纹，
注册失败才启动有上限的恢复重试。支持的内核后端是 notify 的 macOS、Linux/Android、Windows
等原生 watcher，不静默退回 `PollWatcher`。
精确文件的 `files::Source` 在 macOS 使用 vnode 通知并同时观察父目录，覆盖 SQLite
持续打开的 WAL 写入；目录树监听仍使用原生目录 watcher。`ProcessWatch` 通过内核退出事件
观察外部进程，拥有的子进程使用 `Child::wait`；同步平台适配器通过 `Changes::blocking_changed`
和 `io::readable` 等待通知、退出、取消或单次截止时间。当前新增进程适配覆盖 macOS 与
Linux/Android，其他目标没有通过本轮进程验证。

## 时钟的用途

健康订阅没有业务查询定时器。Mesh 每 10 秒可发送缓存的心跳，心跳不调用业务 source；
连续 35 秒没有帧则断开并恢复。连接失败、权威读取失败和文件 watcher 注册失败使用退避。
临时权限拒绝停止数据传输；Station 的控制连接可以重试认证，客户端被确认撤权后仍沿原有隔离流程处理。

操作截止时间、邀请到期、显示中的倒计时、后台保留窗口回收，以及外部身份提供方明确要求的
device-code polling，以及 Synch 既有的发现、反熵维护都有各自语义。配置账号后，外部 Profile
状态和额度仍由 Agent 默认每 60 秒集中采集；结果变化向内部订阅者发布。没有配置账号时仅
等待配置文件通知，不保留该采集时钟。本轮未接入新的上游推送协议，不能把整个系统描述为零轮询。

旧 `watch_peer` / `watch_assignment` 请求继续适配公共 source。手机审批观察需要节点支持
`claim_watch`；不支持时 core 返回升级节点的提示，不恢复成功状态的定时查询。
浏览器需要两端支持新协议：`/browser/events` 只注册订阅，`/browser/receipts` 只提交结果或
撤销；旧 `/browser/poll` 仅保留回执别名，空注册返回 `browser_stream_required`。
命令接收与回执身份持久化；Station 重启后的迟到回执可补全结果，未知结果不会自动重发操作。
接管持久撤销当前 grant，重新授权必须产生新的客户端 ID 与凭据。

## 验证

- `cargo test --locked -p zork-notify`：主题隔离、突发合并、读取竞态、背压撤权、取消、日志追赶和瞬态事件溢出。
- `cargo test --locked -p zork-station -p zork-client-core`：提交边界、文件替换、失败恢复、重复提示及真实空闲超过 30 秒不再拉取。
- `crates/agent-testkit/tests/channel_inputs.rs` 与 `scripts/test-chat-channels.py`：静默输入、按轮中断、配置边界、两个隔离节点的频道收件和重启回执。
- 重建二进制后运行 `scripts/test-sync-idle.py`、`scripts/test-mesh-enrollment.py`、
  `scripts/test-node-tools.py`、`scripts/test-mcp.py`、`scripts/test-shared-services.py`。
  `ZORK_TEST_BIN_DIR` 可固定本轮验证产物；成员测试支持 `ZORK_TEST_ARTIFACT_DIR`。
- 构建 `zork-client-core` 的 `invitation-observer` example 后运行
  `scripts/test-mesh-notifications.py`，验证真实 core 审批观察与浏览器 SSE 协议；
  `scripts/test-native-upgrade.py` / `scripts/test-gateway-upgrade.py` 验证本机升级生命周期。
- 桌面浏览器执行 `crates/zork-gui/tests/test_browser_desktop.py`；
  实际执行结果与未验证范围记录在 本轮报告（本地生成的验收记录）。

单机多节点测试不替代物理跨网、relay 或所有目标平台的现场验证。

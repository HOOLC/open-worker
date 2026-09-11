# Desktop / Android shared client core

桌面和 Android 的客户端协议与会话规则统一到 `crates/zork-client-core`。
桌面保留 GPUI 渲染和专有面板，Android 保留 Compose、JNI 和生命周期适配。

业务状态与变化检测由 `state` 中的共享控制器负责；两端消费其只读订阅。
控制器生命周期、主题合并和 GPUI 渲染边界见 [Client state subscriptions](client-state-subscriptions.md)。

| 共享实现 | 桌面接入 | Android 接入 |
| --- | --- | --- |
| `api.rs`：Gateway HTTP/Mesh 请求、DTO、错误语义、SSE 字节解析 | `zork_gui::api` 重新导出 | core `Client` 使用同一 `GatewayClient` |
| `live.rs`：订阅、重连退避、先订阅后补齐、授权撤销处理 | 核心 `Device` / `Conversation` 消费 `LiveFeed` | 同一核心控制器驱动 |
| `delivery.rs`：持久发送记录与单次投递调度 | 核心 `Outbox` 订阅驱动队列 UI | 同一 `Outbox` 驱动 JNI 状态快照 |
| `state`：业务状态、变化检测、持久化和订阅 | 视图应用核心快照与列表 splice | 会话适配器将同一快照编码为 wire delta |
| `conversation.rs`：活动归并、发送权限、任务修订合并、停止确认 | 由核心控制器调用 | 由同一核心控制器调用 |
| `transcript.rs`：Gateway 消息身份、重放去重、分页与投影 | `zork_gui::transcript` 重新导出 | core 会话适配器使用同一投影函数 |
| `store.rs`：SQLite 草稿、缓存和 outbox 事务 | `desktop::store` 重新导出 | 独立 `LocalClient` 通道 |
| `transport.rs`：client-only Mesh 启动、配置、身份 | GPUI 平台提供执行器和网络配置 | JNI 平台提供执行器和私有目录 |

`subscribe` 立即返回缓存快照，首次连接和断线补齐继续异步进行。事件流先订阅，
再沿历史分页游标回查到上次同步的消息 ID；离线新增超过 100 条时不会只合并
最新一页，已加载的旧历史也会保留。HTTP 和 Mesh 使用同一实现。

`poll` 一次处理一批就绪事件，只在消息有变化时更新历史缓存。空闲返回空对象；
消息变化返回 `messages_upsert` 与 `message_order`，不重复传输未变化的消息正文。
订阅、主动加载旧历史和无 ID 的旧协议消息仍返回 `messages` 全量快照。
设备元数据在独立任务中刷新并合并重复通知，避免阻塞消息和发送状态。
Android 只转换变化的显示记录；返回设备列表时 `unsubscribe` 释放会话，
离开前台时 `pause` 关闭 Mesh。

发送立即写入本地记录并进入消息列表，草稿随同事务清空，不获取网络等待锁。
每次发送只尝试一次，超过一秒仍无回执才显示“发送中”，15 秒超时或请求出错
显示“发送失败”。断线、重连和重新打开应用都不会自动重试；中断的发送转为
失败。用户可手动重发或删除失败记录，重发保留原请求 ID，防止丢失回执导致
重复执行。删除操作仅移除本机的失败记录，不代表撤回服务器已经收到的消息。

同一局域网内，桌面节点和前台 Android 客户端通过 `_zork-mesh-v1._udp.local`
发现当前端口。每次建立新连接前查询地址发现服务，替换重启前的缓存地址；
发现结果只作为路由提示，实际连接仍验证原有设备公钥和成员权限，不自动配对。
Android 的 multicast lock 随连接恢复／暂停获取和释放。显式 offline 模式或
`ZORK_MESH_LAN_DISCOVERY=0` 不启用此发现方式，便于隔离和 relay 验证。

桌面 HTTP 兼容入口仍使用自有 Tokio 执行器；JNI 复用平台已经持有的执行器。
请求、握手和事件流被取消时，子任务随之取消，避免换设备或退出后遗留连接。

两端的布局、导航选择、文件预览呈现和滚动位置由各自 UI 维护；消息、
未读、批注草稿、发送状态及业务缓存由核心维护。

## 验证

- core / JNI：14 项测试，包括消息可见性边界、UTF-8 分片、重连补齐、授权
  撤销、同 ID 重试、工具活动归并，以及网络等待期间的独立草稿写入。
- 桌面：6 项 API/outbox 回归与 3 项嵌入式传输测试。
- Android API 36 arm64 模拟器：6 项真实 JNI/Mesh/Gateway/Agent 集成测试。
  新测试仅调用入队、订阅和状态读取，由 Rust 自动完成发送、投影与缓存。
- APK 校验 v2 开发签名及 16 KiB ZIP 对齐；真机、省电策略和蜂窝切换待验证。

运行日志和 APK 信息位于 `artifacts/android/shared-core-*` 与
`artifacts/android/build-result.json`。构建使用本地临时 Cargo 目录，避免
从 SMB 加载编译期动态库；不改变项目全局 target 配置。

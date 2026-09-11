# Zork Mesh：第一条远端任务闭环

2026-09-05。已实现 Synchronicity v0.1.8（wire 5）接入：A 创建任务并选择 B 的工作区，B 用自己的 Profile 和 Agent 执行，显式消息回到 A 的 Conversation / Inbox，提交文件经过完整 BLAKE3 与长度校验后保存到 A 的 Drive；A 验收后决定传回 B。

验证使用 mini1 上的独立数据目录、身份和真实 Synch 库节点 / Supervisor / Station / Agent / CLI / GPUI，模型采用测试替身。尚未在两台物理设备之间验证 LAN 或 relay，也未接入用户真实设备或模型凭据。截图和执行记录见 验证记录（本地生成的验收记录）。

## 安装与配对

Mesh 默认关闭。每台设备使用自己的 Zork 数据目录，保留原来的配置及 Profile。Synch 已作为 Rust 库嵌入 Zork，无需安装或启动独立 synch 进程：

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 cargo build --locked -p zork -p zork-station -p zork-agent-server -p zork-gh -p zork-gui
```

Cargo 将 Synch 固定到 `6d6283f09c32476dc77c09f76a2b2529a42a558d`（0.1.8）。安装包不再携带 synch 可执行文件，旧 `synch_binary` 配置被兼容读取但不再使用。桌面客户端的远端传输直接运行于 GUI 进程，本机节点由 Station 直接调用 `synch-engine::Node`，同步任务运行在 Station 已有的 Tokio runtime 上。不依赖 `synch-cli`、本地 gRPC、control.sock 或生成的控制协议代码。

在现有 `config.json` 中增加 `mesh` 对象。第一次启动可以将 `peers`、`workspaces` 留空；Station 自己初始化并持有 Synch 库节点，退出时回收后台同步任务、网络和数据库。Station 就绪后从本机 runtime API `GET /v1/mesh` 的 `origin` 读取公钥身份。不要复制另一台设备的数据目录或私钥。

下面是 B 的配置片段；尖括号内容需替换为实际值。`path`、Profile、model、thinking 都由 B 本地配置决定。

```json
{
  "mesh": {
    "enabled": true,
    "offline": true,
    "bind": "0.0.0.0:43120",
    "peers": [{
      "origin": "key:<A 的完整公钥身份>",
      "name": "我的桌面",
      "addr": "<A 的 LAN IP>:43120",
      "execute": ["project"]
    }],
    "workspaces": [{
      "id": "project",
      "path": "/absolute/local/workspace",
      "profile_id": "<B 已有 Profile ID>",
      "model": "<B 已有 model ID>",
      "thinking": "<该模型支持的值>"
    }]
  }
}
```

在 A 的 `peers` 中加入 B 的完整 origin 和连接地址；若 A 只发起任务，其 `execute` 可以为空，`workspaces` 也可以为空。双方通过独立可信渠道核对公钥后配置并重启 Supervisor。`execute` 是授予该 peer 的**本机工作区**执行权限；网络配对不自动授予所有目录。`offline: true` 使用显式地址连接，隔离测试已验证 loopback；跨物理机还需要网络可达性验证。首版不提供账户注册、自动邀请或 relay 设置向导。

GUI 连接 A 的 runtime 地址。在新任务输入框上方选择设备和工作区，再发送目标。首次需要 B 在线发布可用工作区；之后已知列表会缓存，B 离线时仍可排队，实际权限由 B 收到请求时再次验证。远端任务使用 B 的模型配置。B 收到任务后，既有 `chat.post_message(kind="final")` 和 `chat post-file` 会把可见结果与文件送回 A。

更改配置后重启 Supervisor。移除 peer 会撤销 Station 持有的 Synch 节点静态信任，并拒绝新的产品请求；这不撤销已交付数据，也不会自动停止已经启动的本地 Run。需要停止的任务应先发出停止命令并收到确认，再移除设备。

## 持久性与权限

- 每个任务固定一个 owner 和 executor。命令 ID、委派、执行状态、事件游标、验收 outbox 保存于 Station SQLite；同一 ID 的重复请求幂等，不同载荷复用 ID 会被拒绝。
- owner 决定验收/取消，使用任务 revision CAS，并绑定结果消息与产物 ID。executor 不能从本机界面擅自验收；关闭后的迟到事件不重新打开任务。
- B 在投递给 Agent 之前持久保存 `dispatching`。若恰好在 Agent 是否接收未知的窗口崩溃，会标记需要处理，不自动重跑。正常运行中的 Station 重启、ACK 丢失和命令重发不会创建第二次 Run。这不保证任意 shell 或外部 API 副作用恰好一次。
- 远端停止需要 B 的 Agent 确认；断线不算已停止。owner 离线时 B 可继续执行，产品决定等待 owner 恢复。
- Synch socket 仅桥接 Station 的固定 loopback 入口。身份来自 `sy_peer_origin` 和 `sy_peer_device_key`，经过独立本地密钥与 origin/key 对应校验；JSON 中自称的设备身份不能替代认证身份。每个连接只处理一个有界 RPC。
- 产物先保留 B 原有 SQLite 快照，再通过 Synch 发布不可变对象。A 按指定 origin/path 获取，完整校验后保存自己的快照；A 可以在 B 离线、原文件删除后继续读取。当前采用接收时下载，尚未实现仅元数据、按需下载及缓存淘汰 UI。

## 当前范围

一个 Mesh Task 对应一次远端委派；继续工作需要新建任务，暂不支持重开、多轮追加、换执行器、自动故障转移或 owner 迁移。已有工作区文件不会自动成为远端输入；只传递目标和 B 显式提交的输出，不自动同步源码目录、凭据、完整 transcript 或整个 Profile。

单文件沿用 10 MiB 上限。每个 Mesh Task 最多 128 份文件版本，说明最多 16 KiB，消息事件的编码载荷小于 128 KiB；较大结果应提交为文件。配置最多 16 个 peer、32 个工作区。当前不做历史 GC、磁盘配额、多人角色和生产级执行沙箱；获授权 Agent 的执行能力仍取决于该节点既有 Agent 环境。

## 本机 API 与验证

| API | 行为 |
| --- | --- |
| `GET /v1/mesh` | 本机 origin、已配对节点、连通性、对端允许的工作区及委派状态 |
| `POST /v1/tasks/{id}/delegate` | `command_id`、`expected_revision`、`executor_origin`、`workspace_id`、`goal`；保存后后台投递 |
| 现有任务、Inbox、Drive API | 返回导入后的本地投影、远端运行事实及文件快照 |
| 现有取消运行 / 产品 transitions API | owner 请求远端停止；CAS 验收和产品取消通过持久 outbox 送达 B |

这些 HTTP API 沿用本机 Station 的部署边界，不是远端通用 HTTP 代理。节点间只开放明确列举的 Mesh RPC。

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 cargo run --locked -p zork-mesh --example mesh-lab
python3 scripts/test-mesh.py
python3 scripts/test-leader-mesh-ui.py
```

协议与后续方向见 [设计研究](synchronicity-mesh-design.md)。其中的通用事件复制、多轮工作流和文件按需下载是设计目标；本页描述当前实现。

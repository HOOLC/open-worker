# 设备接入 mesh

## 用户流程

1. 在已连接的 Zork 客户端打开「设置 → 设备连接 → 生成加入命令」。
2. 把命令复制到要加入的设备执行。无需在两台设备之间手工交换公钥或配置 Worker 授权。
3. 目标 Gateway 加入后，原客户端显示设备名称和加入结果，侧栏自动出现新设备及其 Leader、任务。
4. 新设备没有模型连接或 Agents 时，在客户端选中该设备后配置；模型凭据不会自动复制到另一台机器。

生成的命令固定使用与构建对应的 GitHub Release 版本。原生安装入口不需要 Node.js 或 npm；脚本检测 macOS/Linux 和 ARM64/x64，下载该平台的完整节点包，校验 SHA-256 后交给原生命令处理接入。

0.1.30 的原生 Release 发布后，界面生成的命令形式为：

```sh
curl -fsSL https://github.com/HOOLC/open-worker/releases/download/v0.1.30/install.sh | sh -s -- --version 0.1.30 -- mesh join '<invitation>'
```

已有兼容原生程序时，脚本直接复用它，也可以运行：

```sh
zork mesh join '<invitation>'
zork mesh join '<invitation>' --data /absolute/path/to/node --name mini2
zork mesh invite --data /absolute/path/to/node
zork mesh status --data /absolute/path/to/node
```

仓库内的 `scripts/join-mesh.sh` 复用同一个安装入口。自动化调用可加 `--json`。版本包、离线安装、兼容性和发布步骤见 [原生分发说明](native-releases.md)。

## 安装与复用

- 默认检查常规节点目录、客户端节点目录，以及已登记的自定义目录。优先复用唯一运行中的 Gateway；多个候选需要明确 `--data`，不会任选一个。
- 用本地控制凭据验证 Gateway 的协议版本和规范数据目录，防止误连占用了相同端口的其他实例。
- 已运行的兼容 Gateway 直接处理加入请求；不替换进程、不复制工作目录、不重置 Profile、Agent、任务或身份。
- 未安装时，将supervisor、直接嵌入 Synch 引擎的 Gateway、Agent 和 GitHub helper 安装到持久数据目录的 `bin`，避免后台服务依赖下载临时目录。新安装使用不冲突的 loopback 端口。
- 已安装但未运行时复用原目录和身份，注册并启动用户后台服务。
- 旧 Gateway 不支持接入协议时，明确要求先更新，保留它正在执行的任务。接入命令不会擅自替换正在运行的旧程序。
- 同一数据目录有 supervisor 实例锁和安装操作锁，重复运行不会创建第二个监听实例。

## 邀请和权限

邀请有效期 15 分钟，仅供一台设备使用，可在界面撤销。Gateway 保存邀请摘要、兑换状态和短期恢复回执。过期或撤销的邀请不能添加新设备；已经加入的同一设备重试只核对当前成员关系。移除设备后，旧邀请不能把它重新加回来。

兑换分两步：先通过固定公钥的加密入口领取挑战，再由目标设备自己的 Synch 身份确认。临时传输授权仅覆盖 `zork-control`，60 秒后到期。只有 Gateway 才能提交成员关系；安装脚本不直接写入对端配对文件，也不携带模型凭据。

通过自己的 mesh 邀请加入的 Gateway 默认允许任务协作和节点管理。Worker 不需要额外登记每个 Leader，工作目录和模型连接仍属于执行设备。客户端身份单独登记为访问端，能够操作获准的节点，但不会被当成执行节点或 Worker。

原有手动配对默认 `collaborate: false`，保留原来的客户端、工作目录和 Worker 授权。自己的 mesh 目前支持最多 16 个 Gateway 和 16 个客户端。

首个创建邀请的 Gateway 维护带版本的成员目录；其他成员可以向它申请邀请或移除设备。新建邀请和成员变更需要该节点在线，现有设备间的任务执行与客户端直连不经过它。当前不提供成员目录维护节点的移除或迁移操作。成员在线时获取更新，离线成员在重连时同步；旧版本目录不能恢复已撤销的授权。

## 访问、执行与连接状态

任务绑定原来的执行设备，客户端切换只改变访问入口。设备、会话和草稿按节点隔离；侧栏保持 Device → Leader → Tasks 结构，任务标题显示执行设备。

断线保留缓存、草稿、阅读位置和最后确认时间，不能从网络断开推断任务已停止。连接恢复后补齐消息和任务状态。尚未发送的消息可撤回草稿；已经开始投递的消息等待同一请求 ID 的回执，不会被误报为成功撤回。停止请求也必须等待实际任务结束状态。

## Gateway 生命周期

客户端与 Gateway 分开安装、分开管理：

| 来源                       | 退出客户端后                   |
| -------------------------- | ------------------------------ |
| 脚本安装的独立后台 Gateway | 继续运行                       |
| 客户端访问已有独立 Gateway | 继续运行                       |
| 客户端启动、未开启后台运行 | 客户端控制连接关闭后停止       |
| 客户端启动、已开启后台运行 | 系统服务接管已有进程并继续运行 |

macOS 使用用户 launchd，Linux 使用用户 systemd，后者需要可用的用户服务管理器。登录后自动启动和保持后台运行是两个独立选项；系统关机或用户服务管理器结束时，仍遵循操作系统的生命周期。

```sh
zork service install --data /absolute/path/to/node
zork service install --data /absolute/path/to/node --at-login
zork service status --data /absolute/path/to/node
zork service uninstall --data /absolute/path/to/node
zork stop --data /absolute/path/to/node
```

`service uninstall` 撤销后台看护，不等于停止现有 Gateway。`zork stop` 同时撤销后台服务并请求停止节点。GUI 关闭后台运行时，先取得客户端控制连接，再撤销系统看护，避免任务重启或所有权空档。

## 验证

- `scripts/test-mesh-enrollment.py`：三个真实 Gateway，命令复用、邀请单次使用、重复执行、成员同步、默认 Worker 协作、移除后的旧邀请拒绝。
- `scripts/test-mesh-experience-ui.py`：真实原生界面，生成命令、自动设备发现、执行位置、离线消息撤回及重连、后台接管、客户端崩溃和小窗口。
- `scripts/test-mesh-installer.py`：当前原生 Release 包与下载脚本 的全新安装、停止实例复用、路径含空格、多实例歧义和旧 Gateway 保护。
- Rust outbox 测试验证取消与开始投递之间的竞争、丢失回执后的持久状态；信号测试验证 GUI 后台线程的信号屏蔽不会传给节点进程。

发布流水线分别构建 macOS/Linux 的 ARM64/x64 release 组件，每个平台测试并打包后，再验证四个平台的完整组件集合和摘要，最后发布 GitHub Release。本机验证不代替其他架构的发布流水线检查。

## Relay 与发现服务（不使用控制面）

设备继续通过 Zork 邀请加入，不依赖 Cue 登录或云控制面。Relay 转发加密流量，discovery 使用兼容 pkarr 的地址发布/查询服务。

桌面端可在 `~/Library/Application Support/Zork/client/services.json` 配置公开端点，也可以用 `ZORK_SERVICES_CONFIG` 指定文件：

```json
{
  "relay_urls": ["https://relay.example.com"],
  "discovery_url": "https://discovery.example.com/pkarr"
}
```

以上是占位地址。显式配置的端点覆盖缓存端点；未提供的字段保留邀请或节点配置。relay 列表整体替换，不与公共列表合并。`offline: true` 仍关闭 relay 和发现；仅填写 URL 不会取消显式离线模式。未配置端点且在线时，使用嵌入式 Synch 的上游默认服务。

桌面传输重建和已停止的本机 Gateway 再次启动时读取服务文件。已经运行的 Gateway 不会因客户端打开而被修改；要立即变更，在该节点的 `/v1/node/mesh` 管理 API 更新 `relay_urls`、`discovery_url`（保留其余配置），或停止后重新启动本机节点。独立安装的 Gateway 使用其数据目录 `config.json` 中的 `mesh` 字段；服务预设不是跨设备自动同步设置。

生成的加入命令包含邀请对应的网络配置。目标 Gateway 没有成员关系和手工 peers 时，即使已经开启 Mesh，也会采用邀请中的网络配置；传输配置变化后等待 Gateway 更新完成再兑换。已有成员关系或手工配对的节点保留其网络选择。模型、Agent、任务、设备密钥和工作目录不会因导入网络端点而重置。

本地验证：`scripts/test-native-installer.py` 检查安装包完整性和复用分支；`scripts/test-mesh-enrollment.py` 验证真实邀请和旧端点替换；`scripts/test-local-relay.py` 检查实际 relay/discovery 流量。后者允许直连升级，不作为“全部文件强制经 relay”或两台物理设备跨网验收的证据。

### 2026-09-07 本地验收

- `cargo test --locked -p zork-config services::`：3 项通过。
- `scripts/test-native-installer.py`：15 项通过（包含模拟四平台包验证，不代表四平台生产包已发布）。
- `scripts/test-mesh-enrollment.py`：真实三 Gateway，旧 relay/discovery 替换、身份及 Agent 进程保留、邀请重复兑换、协作执行和撤销通过。
- `scripts/test-local-relay.py`：真实三节点邀请 + 原生桌面传输通过；discovery PUT 18 / GET 24，relay 收发各 179768 字节。文件检查与撤销通过，但允许直连升级；不宣称强制全量中继。
- `gh release view v0.1.30 --repo HOOLC/open-worker` 返回 `release not found`。当前生成命令对应的公开发布物尚不能下载，本地源码验收不等于发布完成。
- Slack 检索未找到可确认的 Cue 自建 iroh relay/pkarr 端点；[旧部署线程](https://cue-3kl2780.slack.com/archives/C0AQ0C0KVMH/p1787208214758059)记录的是默认 iroh 服务与 DHT 发现。控制面 dashboard/domain 不用作 relay 地址。

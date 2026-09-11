# 原生客户端与 Agent 节点

默认启动 `zork-gui` 进入独立客户端。首次启动不创建节点、不启动 Station、Agent 或 Synch。设置中开启本机节点后，可添加模型连接与 Agents。客户端管理的节点默认随客户端退出；在「节点」开启「退出客户端后保持 Station 运行」后，系统后台服务接管已有进程，任务不重启。登录后自动启动是独立设置。连接已有的独立 Station 不会取得它的退出控制权。工作目录沿用 Station 的 Session 分配机制。
本机节点的 supervisor 只管理一个 Station 子进程，Agent 在 Station 内运行。客户端以 Station readiness 判断就绪；Station 重启时 Agent 同步重启，持久会话在新实例恢复。旧双进程版本首次切换需要完整重启 supervisor。


## 使用

1. 打开设置 → 节点 → 开启本机节点。
2. 在「模型连接」添加 Profile。支持提供商已有的 API Key、device code 和浏览器授权流程；兼容接口可指定模型与 token 上限。
3. 在「Agents」创建 Leader 和 Worker，为 Worker 选择允许指派任务的 Leader。
4. 返回对话，在左侧按「设备 → Leader → 任务」浏览。每台设备有独立连接状态，以及自己的收件箱、任务和文件入口；设备旁的设置按钮进入该设备的 Agent 管理，可切换到模型连接等设置。点击 Leader 继续长期对话，点击其任务查看执行结果；展开箭头只控制列表展开，不切换对话。Leader 的长期 Conversation 不会创建假 Task；每个 Worker Task 有独立 Session，返工复用该 Session。

设备切换保留各自的草稿和会话界面，设备与 Leader 的折叠状态在重启后恢复。任务始终列在发起它的 Leader 下，远端执行设备作为任务附加信息显示。已结束任务默认显示最近三条，可展开全部历史；正在查看的历史任务不会被自动隐藏。设备离线后保留已同步的 Leader、任务、消息和文件，待发送消息仍归属原设备，重连后投递。

添加自己的设备时，在「设备连接」生成加入命令，并在目标设备执行。命令自动查找已有 Station；未安装时安装完整节点组件，未运行时启动后台服务，再调用 Station 兑换短时邀请。存在多个实例时要求用 `--data` 明确选择。重复执行不会生成第二套身份或重复启动服务。

同一 mesh 的 Leader 默认可以使用本机和远端 Worker，不必逐个配置授权。客户端通过已有节点登记自己的访问身份后，会自动发现并直接连接新加入的设备；纯客户端只运行传输组件，不需要 Station 或 Agent。原有手动配对保留原授权，仍可在高级表单添加设备身份和可选 LAN 地址。完整接入、权限与后台服务说明见 [设备接入 mesh](mesh-onboarding.md)。

客户端保存已浏览的消息、任务、文件副本、草稿和阅读位置。离线消息进入持久 outbox；尚未尝试发送的消息可以撤回草稿，一旦尝试发送就显示等待回执，避免把不确定的送达误报为撤回。重连使用同一请求 ID，成功回执丢失时不会重复入队。断线显示最后确认时间，停止请求在收到任务结束状态前保持未确认。任务标题始终标明执行设备，切换客户端不会迁移任务或工作目录。

## 服务配置

发布时使用公开预设，不在源码中猜测 Cue 生产服务地址：

```sh
python3 scripts/package-macos-client.py \
  --services-config /absolute/path/services.json \
  --output /absolute/path/package
```

配置随包放在 `Zork.app/Contents/Resources/services.json`。设备覆盖文件位于 `~/Library/Application Support/Zork/client/services.json`；也可通过 `ZORK_SERVICES_CONFIG` 指定。顶层字段逐项覆盖，`cue` 整体替换，`cue: null` 关闭账号集成。

```json
{
  "relay_urls": ["http://127.0.0.1:3340"],
  "discovery_url": "http://127.0.0.1:6881",
  "cue": {
    "issuer": "http://127.0.0.1:4200",
    "client_id": "YOUR_REGISTERED_LOCAL_CLIENT_ID",
    "redirect_uri": "http://127.0.0.1:43025/oauth/callback"
  }
}
```

以上是本地服务示例，需先运行对应服务；远端 URL 使用 HTTPS。Cue 的 client_id 和精确 redirect_uri 需在发布环境注册。登录使用 OIDC Authorization Code + PKCE，并验证签名、issuer、audience、state、nonce、过期时间与 access token hash。当前接入的是 Cue 身份信息，不会用 OIDC access token 调用 Cue 产品 API，也不把模型凭据混入账号配置。

## 构建与验证

在 mini1 执行，使用锁定依赖：

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 \
  cargo build --locked -p zork -p zork-station -p zork-agent-server -p zork-gh -p zork-gui
```

安装包包含上述二进制；Station 将 Synch 0.1.8 的 `Node` 作为自己 Tokio runtime 中的异步组件，不再携带或启动独立 synch 可执行文件；连接远端节点时，传输直接运行于 GUI 进程，不启动传输辅助进程，默认独立客户端模式；`--host` 是显式旧 SSH 模式。macOS launcher 使用 `ZorkLauncher`，避免大小写不敏感文件系统中覆盖 `zork` 主程序。`zork-call` 已移除，Station 能力注册为动态工具，`zork-gh` 独立保留。PTC 本轮不做。

回归脚本：`test-desktop-node.py`（启动/退出/异常清理）、`test-node-management-ui.py`（原生表单与离线状态）、`test-leader-mesh-ui.py`（多 Leader 头像与三端原生流程）、`test-remote-workers.py`（重启/丢回执/返工/撤权）、`test-client-mesh.py`（纯客户端权限和文件）、`test-cue-account.py`（本地签名 OIDC 服务）。`test-local-relay.py` 使用本地 iroh-relay 1.0.3 和仅用于测试的内存 discovery 服务，验证真实 relay 流量。

当前测试使用隔离假模型和本地服务。真实 Cue 生产账号、生产 relay 的上线验证需使用发布配置；这不由本地 fixture 代替。


### macOS 开发应用生命周期

每个 worktree 保留一份固定的 `.tmp/macos-app.noindex/Zork.app`。

```sh
python3 scripts/package-macos-client.py           # 更新固定 app，不生成压缩包
python3 scripts/package-macos-client.py --launch  # 更新完成后启动
python3 scripts/package-macos-client.py --output /path/to/export  # 显式导出压缩包
```

新包在固定 `.incoming` 暂存位置构建并签名验证，期间不动旧 app 或旧实例。
成功后只退出这个 worktree 槽位内的进程，替换固定 app；原先正在运行则
重新打开新版。未运行时默认不启动。旧进程不退出则保留旧包并报错，不强杀
其他 worktree 或正式安装的应用。锁只串行化更新和测试操作，不禁止运行中
应用被新版替代。失败构建清理半成品，旧 app 保留；替换失败回滚旧包。

平时只有一份当前 app；构建事务期间短暂存在新旧两份，完成后不保留历史
副本。`--output` 是明确导出需求，输出固定名称的压缩包和摘要；重复导出
替换旧压缩包。日常更新不自动生成压缩包。不同 worktree 各有独立槽位。

`--run <命令>` 在替换完成后执行测试，参数中的 `{app}` 替换为当前 app
路径；测试命令结束回收它启动的进程组，但保留当前 app。不要用 `open -n`
让测试进程脱离管理。需要交互开发启动时使用 `--launch`。直接浏览器打包
CLI 同样使用此 worktree 槽位，不能额外保留另一套长期测试包。

断电或 SIGKILL 留下的 `.incoming/.previous` 在下一次持锁更新时恢复/清理。
正式安装目录、用户数据、截图和报告不属于应用槽位替换范围。

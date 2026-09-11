# 服务管理与 Mesh 共享

服务由稳定 ID 标识，启动配置、运行意图和共享开关分别持久化。Gateway 执行进程管理、
权限检查与重启恢复；客户端通过已有 Synch Socket 和本地入口访问应用。

## 工具与 skill 的边界

工具描述定义输入、输出、前置条件、副作用、错误和重试语义。什么时候使用、如何选择
启动方式、就绪验证、应用配置及排障建议放在内置
[`service-sharing` skill](../crates/agent/skills/service-sharing/SKILL.md)，不再塞进基础提示词。
Skill 随 Agent 的版本化 bundled skills 安装，沿用现有发现、读取、禁用和更新机制。

| 工具 | 调用结果与副作用 |
|---|---|
| `service.start` | 创建或恢复当前 Session 的托管服务，保存 argv、cwd、port 和运行意图；不自动共享 |
| `service.attach` | 登记已有的外部服务；不接管进程、不采集它的输出 |
| `service.list` | 列出当前 Session 的服务，包括停止和未共享的记录；不逐个探测端口 |
| `service.inspect` | 返回状态、TCP 就绪情况、进程 PID、最近退出码/错误、revision、链接和日志路径 |
| `service.restart` | 按保存的配置重新启动托管进程，保留 ID、链接和日志 |
| `service.stop` | 停止托管进程组并持久化停止意图；Gateway 重启不会再次启动它 |
| `service.share` | 开启现有服务的长期 Mesh 访问，返回稳定链接；不会启动进程 |
| `service.unshare` | 关闭共享并断开连接；保留服务身份、配置、进程和日志 |

变更工具带 `request_id`。同一 Session 内，相同 ID 与相同参数重试不重复执行；换参数会冲突。
操作意图与请求凭据在一个 SQLite 事务中提交，旧请求的重放不会撤销后来的停止或取消共享。
`service.start` 重用同名记录；修改已有启动配置需要当前 `expected_revision`。
外部服务不能通过 restart/stop 接管。工具操作限定当前执行节点和 Session，由运行时提供身份。

旧 `service(action=share/list/unshare)` 保留兼容入口。旧数据库中的共享记录迁移为外部服务，
保留原 ID 和已共享状态；不猜测它与某个后台 job 的关系。新的正常入口使用上表工具。

## 持久化与进程生命周期

元数据和请求凭据位于节点数据目录的 `state/shared-services.sqlite`。没有八小时或十二小时
服务时限。关闭浏览器视图不改变运行/共享意图；Gateway 重启自动恢复期望运行的托管服务。
端口暂时不可用不会删除登记。停止服务后，只有新的显式启动/重启操作才会恢复运行意图。

每次启动由一个轻量进程守护者拥有应用进程组。Gateway 停止或异常退出时，父进程管道关闭，
守护者终止并回收进程组及后代，避免旧服务占用端口。进程退出和启动错误以结构化状态保存，
不用解析日志判断。失败进程不进入紧密重启循环；显式 restart 或下一次 Gateway 启动再尝试。

日志捕获使用有背压的有界字节缓冲区，stdout/stderr 分开写入：

```text
<节点数据目录>/services/<服务ID>/logs/stdout.log
<节点数据目录>/services/<服务ID>/logs/stderr.log
```

每个当前文件最多 4 MiB，分别保留 `.1`、`.2`、`.3` 三份轮转文件，即每服务最多约 32 MiB
输出日志。停止、重启和取消共享不清空日志。每次启动的结构化状态文件单独保存，保留最近八份。
`service.inspect` 返回拥有节点、目录和文件路径；没有 `service.logs`、日志分页 read 或日志 RPC。
Agent 在日志所属节点复用文件读取、搜索和 shell 工具。返回的路径不代表日志已公开或自动发布到 Mesh 文件空间。
外部服务的日志由其原管理器负责，inspect 不伪造捕获路径。

## 浏览器和传输

共享链接为 `zork://service/<节点公钥>/<服务ID>/<路径>?查询#片段`，不是绕过成员授权的令牌。
客户端建立 `http://s<节点与服务摘要>.localhost:<临时端口>/` 的 loopback 入口，并在内置浏览器打开。
桌面消息/地址栏和 Android 消息支持这个入口；不需要公网 HTTP Gateway 或手机 CLI。

每条连接复用 `zork-control/mesh.sock` 的认证前导，完成有界 JSON 鉴权后转为原始双向字节流。
HTTP body、资源与 WebSocket 不经过 JSON 编码。服务端核验共享状态、当前 client 权限以及控制端口限制；
托管服务还必须处于期望运行且进程存活状态。撤权、停止、重启或取消共享会关闭相应已有流。

每个服务使用独立的 localhost 子域隔离 Cookie，入口检查 Host、Origin 和 Fetch Metadata。
Android 仅向 localhost 域放行明文 HTTP，远端一跳由 Mesh 加密；WebView 不暴露原生 JS bridge 或本地文件访问。
当前上游是 HTTP；应用写死的 localhost URL、绝对重定向、Cookie Domain 与 HMR 端口不自动重写。
本地存储还按客户端临时端口区分，重开页面不保证保留 localStorage。跨站 OAuth 流程需要单独适配。
Synch v0.1.8 默认在 300 秒无字节进展后结束流，应用 WebSocket 需要心跳或重连。

## 验证

按构建环境规则重建 Gateway 等进程二进制及 `service-listener` example 后，运行
`scripts/test-shared-services.py`。可配置 `ZORK_SERVICE_CHROMIUM` 和 `ZORK_PLAYWRIGHT_MODULE`
加入真实 Chromium 检查。覆盖 Agent 命名工具、持久化、进程启停/重启与异常恢复、日志文件、
重试、权限、HTTP/大文件/WebSocket、取消共享与进程组清理。它使用隔离身份与 fixture 模型，
不代表物理手机、蜂窝网络或强制公网 relay 的验收。

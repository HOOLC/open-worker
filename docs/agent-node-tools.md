# Agent 的 Device、MCP 与 Skill 工具

2026-09-10。用户通过对话提出需求，Agent 用三个工具域完成设备上的准备、能力安装和使用方法配置。无需用户自己执行 Zork CLI。

## 一条完整流程

> 把这个 MCP 安装在桌面节点，再给 Research Agent 配上使用它的 skill。

1. `device.list` 发现可管理的设备，`device.inspect` 查看目标设备的系统与运行程序。
2. `device.exec` 在目标设备准备依赖或文件；通过 `device.status` / `device.read` 确认命令完成。命令中的路径属于目标设备。
3. `mcp.install` 注册 MCP 配置，`mcp.probe` 验证连接，`mcp.inspect` / `mcp.call` 验证实际能力。
4. `skill.import` 导入目标设备上的 skill 目录，或 `skill.install` 安装 manifest 与资源包。
5. `device.agents` 选择该设备上的 Agent；`skill.bind` 添加 skill，保留 Agent 已有的其他来源。
6. 目标 Agent 使用 `skill.list` 确认新 skill 已生效，再按其中的说明调用 MCP。

Agent 的模型接口仍是现有 `call`；这里列出的是它调用的逻辑工具名。`tool.help` 返回当前工具的 TypeScript `type Arguments` 与字段注释，描述必填/可选字段、联合类型和长度/数值约束；这份说明由工具参数定义统一生成。

## 通用标识与权限

- `target` 是 `device.list` 返回的 Station 身份，不是显示名称。省略表示当前执行节点，也可显式使用 `local`。`cwd`、导入路径和程序路径都属于 target 所在设备。
- 新操作回执使用 ULID `operation_id`。MCP 服务使用 `server_id`，托管 skill 使用 `skill_id`；旧 MCP 的 `server_ref` / `call_id` 仍兼容。
- Device 执行与 skill 安装、绑定、复制使用节点管理权限；远端由目标 Station 检查调用节点的有效成员身份与现有 `peer.client` 授权。
- MCP 管理使用同一节点管理边界；MCP 工具调用继续使用自己的服务 grant。能调用某个 MCP，并不意味着能在其设备上执行命令或管理 skill。
- 调用身份从本地 ToolContext / Session 解析，跨 Mesh 使用认证 Station 身份。模型不能填写另一个调用方或冒充另一个 Session。
- Device 操作状态和输出属于创建它的调用方 Session；跨 Session 获取相同 operation_id 会被拒绝。

## Device 工具

| 工具 | 用途 |
| --- | --- |
| `device.list` | 列出可管理的设备及未响应/未授权节点 |
| `device.inspect` | 查看 OS、架构、可用程序与托管工作区位置 |
| `device.agents` | 查看目标设备上的 Agent ID、名称与角色 |
| `device.exec` | 执行用户授权的命令，立即返回持久操作回执 |
| `device.status` | 查询 operation_id 的状态和结果 |
| `device.read` | 分页读取进程输出 |
| `device.cancel` | 请求中断仍在运行的操作 |
| `device.recover` | 重新取得丢失的原操作回执 |

`device.exec` 默认工作目录按调用方 Session 隔离，位于目标节点的 `device-workspaces` 下。可显式传入目标设备的绝对 cwd。它已经是异步执行，不要再用 `&` 或 daemonize 把子进程脱离跟踪。

默认时限 600 秒，可设置 1–86400 秒；单节点最多 8 个同时执行的操作。单条命令输出上限 8 MiB。`device.read` 每次最多返回 32 KiB，base64 保留原始字节，text 用于预览；继续读取使用 next_offset。

Station 在派发前保存状态。正常结束保留 PID、cwd、退出码和输出信息；取消、超时及撤权先终止并等待所拥有的进程，再保存终态。`cancelled` / `timed_out` 且 `result.process_state=exited` 表示进程已退出；派发前取消为 `not_started`。这些状态不表示已经发生的副作用被回滚，`effects_may_have_occurred` 明确标记这一点。Station 被强杀时不能保证任意外部进程已退出；重启后的在途操作标为 outcome_unknown，不自动重新执行安装脚本。先检查状态和实际效果，再决定后续动作。这里没有增加 OS 沙箱，也没有提供 Station 自身升级/重启或 Mesh 成员授权变更工具。

## MCP 工具

主入口是独立的 `mcp.*` 工具：

- 管理：`installed`、`configure`、`install`、`probe`、`update`、`enable`、`disable`、`share`、`uninstall`。
- 使用：`search`、`inspect`、`call`、`status`、`read`、`cancel`、`recover`。
- `mcp.setup` 和旧 `mcp(op=…)` 保留兼容；通用设备发现和准备使用 `device.list` / `device.inspect` / `device.exec`。

新入口使用 target + server_id。`mcp.inspect` 与兼容 `mcp(op=inspect)` 把第三方参数定义转换为 TypeScript type + 注释；Station 内部协议定义、校验及 binding_revision 不变。MCP status/read 同样把查询成功与实际调用结果分开。`mcp.call` 返回的 operation_id 与旧 call_id 是同一个 ULID，查询使用 `mcp.status`。配置变化检查 expected_revision；具体工具调用检查 inspect 返回的 binding_revision，两种 revision 不互换。

配置、协议兼容范围、凭据引用和 MCP 专用恢复行为见 [Mesh MCP](mesh-mcp.md)。

## Skill 工具

已有工具继续保留：`skill.list`、`skill.sources`、`skill.validate`、`skill.write`、`skill.archive`、`skill.bundle`。它们用于当前 Agent 的发现/来源和已有技能文件编辑，以及内置技能版本管理。

新增面向设备的托管包工具：

| 工具 | 用途 |
| --- | --- |
| `skill.install` | 安装 SKILL.md 与资源文件组成的包 |
| `skill.import` | 从目标设备上的目录导入包 |
| `skill.installed` | 分页列出目标设备的托管包与内容 revision |
| `skill.export` | 读取完整包；可用 expected_revision 固定快照 |
| `skill.share` | 将固定 revision 的包从 target 复制到 destination |
| `skill.bindings` | 查看哪些 Agent 绑定了该包；归档后仍可查询，package_state 标记 installed/archived |
| `skill.bind` / `skill.unbind` | 为明确的 Agent 添加/移除该包来源 |
| `skill.uninstall` | 归档已解绑的包，保留资源 |

`skill.install` 的 package 格式：

```json
{
  "content": "---\nname: example\ndescription: Example workflow\n---\nInstructions here.\n",
  "resources": [
    {"path": "scripts/helper.py", "base64": "cHJpbnQoNDIpCg==", "executable": false}
  ]
}
```

每个包最多 32 个资源，序列化 JSON 最多 96 KiB；导入不收集隐藏条目，资源必须是普通文件，拒绝符号链接和路径逃逸。较大的依赖应在设备上单独准备，不打进 skill 包。

安装后不会自动提供给所有 Agent。包保存在该节点的 `managed-skills/<skill_id>`，绑定修改指定 Agent 的额外来源，其他来源与模型配置保持原样。绑定/解绑/卸载要求当前 expected_revision。仍有绑定时卸载返回 skill_still_bound，先通过 bindings 查看并解绑选定 Agent。

跨节点分享由调用 Station 分别获得源、目标节点授权，再传递固定包快照；不把源节点身份当成调用方，也不让它代用对其他节点的权限。复制后的包有新的 skill_id，并需要在目标设备单独绑定。

## 回执和恢复

Device 与托管 skill 操作统一返回 `operation_id / target / state / result`。`device.status` / `device.read` 成功表示查询或读取完成，即使被查询操作为 failed/cancelled；操作结果由 state/result 表达。已确认终止的取消与超时会清除未完成条目，不需要 recover 或额外确认 outstanding。这些操作用 `device.status` 查询；管理 RPC 的接收和实际完成分开。文件或绑定变更过程中发生崩溃时可能返回 outcome_unknown，可通过 installed / bindings 核对实际结果。

发送方持久保留投递记录，`device.recover` 重用原 invocation_id 和已保存的包快照，避免重试时重新读取变动的源目录。明确的首次权限拒绝不会变成等待将来授权的任务。每节点操作记录最多一万条，发送方 outbox 正文预算 32 MiB；达到上限明确拒绝新操作。

不同逻辑工具共享一份持久状态：`device.exec` / `device.status` 与托管 skill 操作共享 device 状态；`mcp.call` / `mcp.status` 与兼容入口共享 MCP 状态。底层 `ToolCompatibility.state_namespace` 显式指定仍注册的状态所有者，原有工具默认行为不变。这样状态查询完成后能清除原调用的未完成条目，快照和重启也保持一致。

## 验证

按构建环境规则重建 zork、zork-station、zork-agent-server 和 zork-gh 后运行：

```sh
python3 scripts/test-node-tools.py
python3 scripts/test-mcp-management.py
python3 scripts/test-mcp.py
```

测试使用隔离数据目录和真实 Station/Agent/Mesh 进程，模型和 MCP 服务为 fixture。覆盖目标设备执行、回执去重、输出及 Session 隔离，MCP 命名工具，以及带资源的 skill 导入、绑定、复制、解绑与归档。另有共享工具状态的快照回放和操作恢复单测；本机双节点测试不等同于物理设备跨网验收。

### 真实模型验收

`scripts/test-node-tools-live.py` 接收显式指定的 Profile 和模型，运行真实模型驱动的三阶段验收：远端准备并安装 MCP/绑定资源 skill、目标 Agent 按新 skill 调用 MCP、取消命令并复制/归档 skill 和卸载 MCP。脚本通过文件、调用记录和持久事件核验结果，不把模型自述当作通过证据。

```sh
python3 scripts/test-node-tools-live.py \
  --profile /path/to/profile.json --model grok-4.6 \
  --context-tokens 500000 --output-tokens 8192 \
  --output artifacts/node-tools-live
```

模型上限参数仅在旧 Profile 未声明 limits 时用于补齐测试副本。测试复用有效访问凭据，不在测试副本中轮换原账号的 refresh token。报告移除凭据；退出时停止测试节点并清理临时目录。MCP 服务是受控 fixture，Station、Agent、Mesh 和模型调用均为真实运行；两个节点仍在同一物理主机。

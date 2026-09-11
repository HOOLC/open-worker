# 工具生命周期设计复查

2026-09-10，初次代码审查及随后修正。下文保留当时发现的问题；当前实现已完成以下调整，验证记录见 `artifacts/grok46-node-tools-20260910/`。

- `device.exec`、MCP call 和托管 Skill 变更以实际业务终态完成普通工具 Future；内部回执、去重和恢复记录保留。事件订阅传递完成与输出，断流按原回执和已落盘偏移重连，不向模型提供主动轮询流程。
- `tool.cancel` 通过原 invocation 传播，执行器持有并发名额直到清理结束。源端与目标端保存取消标记；取消早于排队接收时，后到的同一请求不能启动。设备命令及 MCP stdio 进程先终止并回收；MCP 外部副作用无法确认时明确返回 outcome_unknown。
- 核心执行器默认无执行期限，删除 shell/device 的模型执行期限参数及 MCP 整段调用期限；HTTP 只保留连接建立期限。满载在内部等待名额。
- service、browser、agent.assign/rework 的 request_id 由 invocation_id 填写。实体 ID、修订检查及长期资源查询保留。
- 设备输出和大 MCP JSON 落到调用会话的 `.zork/live-<invocation_id>.log`，模型用 file.read；大结果分块传输，原生图片仍以图片附件交付。
- 新目录隐藏旧 mcp/service 总入口及设备/MCP 回执轮询入口。新 named 工具使用 result schema v2，兼容层不再从业务列表重建 outstanding；v1 事件与旧快照继续保留和处理历史回执，不能通过迁移抹去未知副作用。
- 已删除 device.jobs 的工具注册、Gateway 分发和列表查询实现；执行回执仍用于内部去重与恢复。
- 普通 pending 通知包含可供 tool.cancel 使用的 invocation_id，明确说明仍在运行，并作为正常通知交付。

## 初次审查记录

## 优先修正

1. **MCP 调用和 Skill 变更也有二次异步。** `crates/gateway/src/mcp/calls.rs` 启动后台调用后即返回 call_id；`crates/gateway/src/node_tools/mod.rs::submit` 把 skill.install/import/bind/unbind/uninstall 包装成独立后台操作。Agent 工具 Future 因此提前完成，模型还得调用 status。应以实际业务操作结束作为普通 ToolResult 的完成点。
2. **提前完成绕过框架的取消和并发管理。** `crates/agent/src/session/executor.rs` 在工具 Future 完成后释放 permit 并移除 live invocation，而远端命令仍在运行。标准 tool.cancel 或会话停止不能通过这个已结束的 invocation 取消远端操作。应把远端生命周期、取消传播和完成通知接入同一个调用，保留内部去重与恢复记录。
3. **执行时限多层叠加。** `crates/agent/src/session/service.rs` 默认一小时；device.exec 默认 600 秒；MCP 调用被 120 秒整段 timeout 包裹。Gateway 工具 HTTP client 还有整请求 30 秒上限，改成真正 await 后也会截断长操作。业务执行应到完成或明确取消；连接建立、传输断线检测、数据库锁等待、终止确认等资源保护需分别处理，不能混作命令总时限。
4. **协议恢复与幂等细节交给模型。** service.*、browser、agent.assign/rework 要模型填写 request_id，device/MCP 又暴露 recover。运行时已有 invocation_id，chat.post_file 已用它自动填写请求标识。投递去重、重连与原调用回执恢复应由适配层负责；需要用户决定是否重做未知副作用时，才返回明确的不确定结果。
5. **重复维护未完成集合。** `NodeState` / `McpState` 又跟踪 accepted/running/outcome_unknown 并增加 outstanding。它们主要补偿工具提前返回，导致执行器和业务状态各自维护完成条件。新调用应使用框架自身的 pending/completion；历史不确定结果仍需保留，不能靠清状态或自动重跑迁移。
6. **大输出读取另造接口。** device.read 让模型处理 operation_id、base64 和字节分页，mcp.read 又有另一套。shell.run 已把输出写入稳定日志并通过 file.read 读取。应复用文件/流输出抽象，由远端适配层处理传输与分页；原生图片等类型化结果继续保留。
7. **明确未接收与投递未知混在一起。** 目标节点满载在 accept 前返回 device_busy；源节点 api 的错误分类仍将其写成 pending_delivery 并要求模型 recover。容量等待、明确拒绝、回复丢失需要分开，避免把已知尚未执行的请求说成副作用未知。框架/目标节点的背压应在内部处理。

## 另一个值得收敛的接口问题

Agent 同时看到旧 mcp(op=...)、新 mcp.*、mcp.setup 和 device.list/inspect；两套入口还分别使用 owner/server_ref/call_id 与 target/server_id/operation_id。真实 Grok 测试已出现同一流程混用两套入口。兼容协议可以保留，但新 Agent 的工具目录应只介绍一种规范入口。

## 不应机械删除的业务概念

- service 是长期运行的资源，service.inspect/stop 有业务意义；service.start 不应等服务退出，但应明确完成条件究竟是进程已启动还是已就绪。
- agent.tasks 是委派任务的业务查询，不应仅因返回状态就判为轮询错误。
- server_id、skill_id 等实体标识，以及 expected_revision/binding_revision 的并发和定义一致性检查，需要保留。
- 跨节点去重、持久回执和认证是必要的适配层职责。应该隐藏协议负担，不是删除可靠性机制。
- 浏览器客户端内部的等待/投递机制不等于模型主动轮询；该实现已经在 command 中等待回执，应单独审查取消传播和超时边界。

上述建议顺序已执行。浏览器底层协议自身的取消与连接边界不在本次远端 exec/MCP/Skill 生命周期改造范围内。

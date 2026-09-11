# 伙伴 action

模型调用统一的 `call` 时填写 action 外层参数，与 `tool` 和 `arguments` 同级：

```json
{
  "tool": "shell.run",
  "action": "查看提交和未提交改动",
  "arguments": { "command": "git status --short && git log -5 --oneline" }
}
```

`action` 描述这次调用的行为（最多 80 字符）。模型使用用户的语言填写简短描述，不写推理、原始命令、敏感输入或未经确认的成功结论。工具 schema 和运行时要求非空 action，工具专用参数仍放在 `arguments`。气泡不再使用 goal；历史 activity 的 goal 字段在读取时忽略，任务分配等业务参数中的 goal 不受影响。

新的 action 立即替换旧动作。LLM 请求开始后，在尚无输出时上一动作最多保留 3 秒，超时后显示“请求中”。首个输出一到立即显示实时速度，不等待 3 秒保留期结束；推理、正文和工具参数的输出均计入，推理内容不作为聊天正文展示。

工具在注册 `ToolInstance` 时通过 `with_activity` 声明展示方式：

```rust
ToolInstance::new(contract, implementation, compatibility)?
    .with_activity(|args| ToolActivity::field("读取", "Reading", args, "/path"))
```

回调返回中英文动作文案、明确选择的目标，以及可选的 `ActivityTarget::Agent` / `Task` 引用。嵌套参数使用 JSON Pointer，例如浏览器的 `/action/op` 和 `/action/url`。不要把整个参数对象、消息正文、文件内容或页面输入文本用作 activity。未声明映射的工具使用“执行操作 / Working”，不根据工具名或参数字段猜测。

运行时在 `StepCompleted` 的 invocation 中保存 `activity`，因此工具注册变化或重连不会重新解释已保存的动作。该字段是可选的，旧事件仍能回放；没有元数据的旧事件使用通用文案。注册回调和回退文案不进入模型的工具 schema；模型只填写 `call` 的 `action` 参数。

Gateway 解析伙伴名称和任务标题，将 `labels` 和 `detail` 放入 status 的每条 call。未知引用省略目标，不把内部 ID 当名称显示。客户端选择语言并展示，不维护工具名匹配表。旧客户端仍可忽略新增字段。

未完成工具按 invocation ID 保留，新步骤添加工具，结果仅移除对应调用。思考与工具执行可以并存，`tools_started.thinking` 表示此时模型也在运行；显式等待不会因为其他后台工具完成而消失。正常结束、取消和失败仍使用现有终态规则。

气泡沿用点击伙伴头像打开历史的交互，不增加操作详情入口。

气泡宽度以每秒 1440 个逻辑像素（Android 为 dp）线性变化，包括展开、收起和文案替换；变化期间重新设定目标时从当前宽度继续。头像位移和文字透明度保留原动画，减少动态效果时直接到达目标。

文字使用实际字体排版测量，右侧 padding 固定为 12 个逻辑像素，截断时仍保留。桌面只在文案变化时重新测量；Android 复用文字测量缓存。每帧只采样动画位置，静止后停止刷新。


显示状态机位于 `crates/zork-client-core/src/activity.rs`。Gateway 适配 durable 步骤事件与 transient 输出统计，调用同一个 Rust reducer，并以 `state: live` 发出完整 `presentation`。桌面直接使用 core 文案，Android 使用 core 序列化的本地化文案；两个客户端不实现计时或速度算法。新版 Gateway 的 `live` 状态需要配套客户端。

输出统计涵盖正文、推理片段与工具参数，但只传递字节计数，不泄露推理正文。供应商目前仅在结束时给出 token usage，实时速度按 UTF-8 字节数除以 4 粗估，并始终显示 `≈`，不是精确 tokenizer 计数；无输出能力的供应商不会伪造流速。速度采用 2 秒窗口，停顿降为 0；首个输出立即通知，后续输出统计每 250 毫秒合并。core 提供下一次显示期限，旧动作到期无需新事件；无变化时停止计时与状态推送。步骤 ID 防止迟到输出污染下一次请求，采样桶数量固定有界。

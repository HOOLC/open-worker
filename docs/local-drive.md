# 本地 Drive 与任务文件

2026-09-09：领队会话上传、扇形附件预览、委派携带文件和 `chat.post_file` 的新消息行为见 [Conversation 附件](conversation-files.md)。以下保留原任务产物 API 的兼容性说明。

2026-09-05：任务文件从占位入口接入本机 Gateway 和 GPUI。

## 使用方式

在任务中打开“任务详情”，滚动到“任务文件”，点击“添加文件”。选择此任务工作区内的普通文件，单个文件上限 10 MiB。保存的是提交时的独立快照；原文件之后修改或删除，不会改变已提交的版本。

Agent 可以使用已有的会话绑定 CLI：

```sh
chat.post_file({"file_path": "/absolute/workspace/report.md", "initial_comment": "交付报告"})
```

本地 GUI 会话将文件登记到所属任务；原有 Slack 文件投递路径保持原行为。这里不会发送消息给外部联系人。CLI 的现有会话坐标和目标会话校验仍然适用。文件出现在任务详情和 Drive 中，caption 显示在文件预览；不会把内部 Agent transcript 转成聊天消息。

Drive 左侧按任务筛选，右侧列出全部文件版本。点击文件预览 Markdown、最多 256 KiB 的 UTF-8 文本，以及 PNG/JPEG；其他格式或较大文本提供“另存副本”。保存使用原生路径选择器，按选定路径写入快照。预览能返回所属任务，任务详情也能打开指定文件版本。

宽窗口并列显示列表与预览；900px 窗口中预览使用内容区，关闭预览回到文件列表。沿用 Cue 的 320px 侧栏、44px 标题栏、Inter 字体及原始 File/Download 图标。中英文切换覆盖文件列表、操作、空状态与错误提示。

## 数据与一致性

新增 `task_artifacts` 表，文件字节、任务 ID、提交时的工作区、相对路径、类型、版本、caption 和提交时间在同一个 SQLite 事务内保存，继续使用原来的 `gateway.sqlite`。没有第二套文件目录或中心服务。列表仅查询元数据，字节按文件 ID 读取。

同一任务内，同一路径再次提交时，若字节和 caption 与最新版本相同，返回原文件 ID；变化则增加版本，旧版本不可修改。新增版本推进任务 revision，因此旧页面不能在未刷新时验收一组已变化的交付文件。已完成或取消的任务拒绝新版本；完全相同的重试保持幂等。文件提交本身不触发结果验收，仍需显式 final 消息。

路径以任务工作区为边界。支持 macOS `/var` 与 `/private/var` 的父目录别名；相对路径不能包含父级跳转。文件读取通过目录描述符逐级打开，并使用 `O_NOFOLLOW` 防止验证后路径被符号链接替换到工作区外。只接受普通文件，读取前后检查大小与修改时间，超过上限或读取期间变化的文件不登记。

## API

| 接口 | 行为 |
| --- | --- |
| `GET /v1/artifacts` | 所有任务文件版本的元数据，按提交时间倒序 |
| `POST /v1/tasks/{task_id}/artifacts` | `{ "path": "/absolute/workspace/report.md", "caption": "可选说明" }`，返回 `{ "ok": true, "artifact": ... }` |
| `GET /v1/artifacts/{artifact_id}/content` | 返回已保存的原始字节，下载响应使用 attachment 与 nosniff |
| `GET /v1/tasks/{task_id}` | 现有任务详情增加 `artifacts` |
| `POST /chat/post-file` | 对本地 GUI 会话复用现有 filePath / initialComment 参数，返回注册的 artifact |

任务、文件元数据和内容均可在 Agent 离线时读取。注册文件只要求 Gateway 与工作区可读，不要求 Agent 在线。Gateway 离线时 GUI 保留上次列表和已加载的预览，并显示重试提示。

## 当前边界

这是任务产物库，不是整个工作目录的文件管理器。不会自动扫描目录、跟踪未提交文件或执行文件。已通过 [Mesh](local-mesh.md) 支持配对节点的任务输出传输与接收端离线快照；尚未实现共享链接、通用文件同步与冲突、删除/回收站、分页、压缩去重、磁盘配额及 PDF/Office 内嵌预览。每个版本都保留完整字节；大量大文件会增大 SQLite 数据库。

当前安全文件读取实现面向 Unix，覆盖 macOS/Linux；Windows 注册暂不支持。文件选择器所在机器需与 Gateway 能访问同一工作区，不能将本版理解成跨设备上传协议。

## 验证

数据库测试验证版本、字节幂等、重启、原文件删除后读取、关闭任务拒绝变更、旧验收版本冲突、路径越界、符号链接与大小限制。GUI 单元测试验证另存时替换目标文件及失败时清理临时文件。

`test_drive_artifacts.py` 使用真实 Agent、Gateway 和动态工具，验证文件提交、会话目标校验、版本、服务重启和 Agent 离线读取原始字节。原生 GUI 回归覆盖文件列表、版本预览、中英文、重启和任务文件双向跳转。

```sh
python3 crates/zork-gui/tests/test_drive_artifacts.py
python3 scripts/test-desktop-headless.py
```

原生客户端截图（本地生成的验收记录）。测试使用隔离工作目录，不使用用户真实任务数据。

# 页面、应用与运行资源

桌面保留现有导航、聊天区、输入框与设置布局，用各自的使用场景承载入口：

- 对话标题栏的「文件与页面」按页面、文件两组展示该对话内容，每组最多预览最近 3 项，窄小窗口进一步缩减。每组的「查看全部」复用右侧独立标签页；完整列表支持搜索与虚拟滚动，切换标签、收起面板和关闭预览后保留筛选与阅读位置。旧 Worker 任务的明确交付会关联到所属领队；其他对话、工作区文件和服务清单不会自动进入这里。
- 页面消息中的链接与浮层条目都能在现有右侧浏览器打开。相同会话中再次打开同一交付 URL 会选中已有标签，HTTP 重定向后仍保留关联；切换标签和收起浏览器保留页面状态。
- 浏览器空白起始页展示所有已连接设备发布的「应用」。应用是独立、持久的发布记录；服务共享、服务运行时长和一次页面交付都不会自动发布应用。
- 现有 Agent 编辑器展示其实际生效的 Skill 清单，可阅读正文并进入附带文件；不局限于托管安装清单。
- 设置中的「工具连接」汇总 MCP 连接，详情读取真实工具说明与参数。失败调用的详情可以按结构化调用身份进入对应连接。
- 现有设备概览展示该设备的服务，详情包含状态与有界日志读取。全局「Mesh 资源」导航已撤下。

## 页面与交付契约

`chat.post_page` 复用频道投递链路，要求显式 `chat_id`，并支持既有 `target` 与 `chat.recover`。Agent 身份、执行 Session 与去重 invocation 由运行时提供。链接消息、作者、接收通知和对话页面索引在同一个 Gateway 事务中提交，不能把页面交付到 Agent 的内部控制上下文。旧会话的 `/v1/pages` 交付接口及 Mesh 任务事件保留兼容路径。

`page.publish` 创建或更新全局应用；`page.unpublish` 撤下同一执行 Session 发布的应用。取消发布不停止服务、不删除已交付的对话引用；延迟重试旧的 publish 不会恢复已取消的应用。HTTP(S) 或有效 `zork://service` URL 经规范化后决定页面身份，不接受可执行协议或内嵌账号凭据。

Gateway 保存页面引用、发布记录与操作回执。`GET /v1/node/pages` 及现有 catalog 同步 Resource 记录提供客户端投影。未绑定的旧 Mesh 会话先保存引用，在会话绑定后才发布客户端索引；事务失败不会留下半条引用。客户端持久副本保存应用和页面关系，重新启动时可恢复；运行设备离线不会删除已发布应用。

## 详情与边界

`GET /v1/node/resources` 使用现有节点管理员认证，只对已授权 Mesh 客户端开放。返回公开摘要，不返回 MCP 启动配置、命令或凭据。Skill、MCP、Service 的读取问题分别报告，读取失败保留上次成功结果，撤权清空旧内容并隔离在途旧响应。

详情接口：

- `/v1/node/agents/{id}/skills/catalog`：当前 Agent 按实际来源发现的 Skill。
- `/v1/node/agents/{id}/skills/{skill}?file=…`：正文或相对文件，只允许包内普通 UTF-8 文件，拒绝路径穿越和符号链接，最多读取 128 KiB。
- `/v1/node/resources/mcp/{id}`：按需连接并读取工具定义，保留停用、认证和探测错误。
- `/v1/node/resources/service/{id}?log=stdout.log`：注册服务详情及 stdout/stderr 的末尾最多 64 KiB，不提供任意文件读取。

`zork-client-core::resources` 共享按需控制器、缓存、读取状态与连接 generation。详情缓存最多 64 项；切换或撤销连接后，旧响应不能恢复之前的内容。GPUI 只保留选中、详情层级、焦点、滚动和解析后的显示缓存，通过 `FrameDelivery` 准备并应用快照后确认版本。打开页面或手动刷新触发读取，没有业务轮询。

对话内容索引与全局应用归并由 core 生成。全局应用源只保留发布记录；不因每条聊天消息重新聚合完整页面目录。对话文件/页面、全局连接、应用与工具列表按可见行绘制。Skill 文件和日志使用现有只读文档组件，原生页面和设置控件保持共享视觉实现。

## 验证入口

```sh
eval "$(python3 scripts/lib/build_env.py --shell)"
cargo test --locked -p zork-client-core --features desktop --lib resources
cargo test --locked -p zork-client-core --features desktop --lib pages
cargo test --locked -p zork-gateway --bin zork-gateway pages
cargo test --locked -p zork-gui --features headless-bench --test headless_resources
cargo build --locked -p zork -p zork-gateway -p zork-gui
python3 scripts/test-mesh-resources-ui.py
```

原生脚本需要可用的桌面和完整浏览器运行组件，可通过 `ZORK_BROWSER_RUNTIME` 指向已构建的组件。产物在 `artifacts/resource-native-implementation/`。实际报告分别记录数据库/core 行为、原生控件、同机隔离进程和 100000 条混合消息的 CPU 回放；不将这些结果当作物理多设备、Android 界面或显示器 FPS 的验收。

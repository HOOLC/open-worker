# Profile 额度显示

设备的“大模型”列表显示额度摘要；连接详情显示供应商实际返回的各周期剩余比例、重置时间、余额及更新时间。时间采用相对表达，例如“额度更新于 5分钟前”“2小时后重置”；依据真实时间戳计算，不显示绝对时间或 UTC。额度更新时间位于刷新额度按钮旁，明确表示额度快照的时间。

- 没有额度、未查询、供应商不提供额度时，不显示额度区域。
- 查询失败时只显示“查询失败”。供应商原始错误不进入额度 UI。
- 0% 和零余额仍然显示；未知额度不当作 0 或无限。
- API Key 探测中的历史 `unlimited` 标记不能用于显示无限余额；只有带单位的真实数值余额才显示。
- 账户状态与额度状态分别读取。仅配置凭据、未实际验证的兼容 API 连接不会显示“已验证”。

## 数据路径

`zork-profile` 的供应商探测保存 `account`、`rateLimits` 和 `checkedAt`。`zork-client-core` 保留这些字段并归一化额度窗口；桌面视图在数据或语言改变时生成展示文本，绘制时不解析 JSON 或日期。

OpenAI/Codex 订阅读取 `/backend-api/wham/usage`，包括主窗口、次窗口和额外限制。Claude 订阅读取 `/api/oauth/usage`，包括五小时、七天和模型专属七天限制。xAI 沿用已有额度查询，预付余额标明 USD。OpenAI 返回的额外余额以 credits 显示，不推断为美元。

后台沿用默认 60 秒的状态刷新。额度页面可见时每 60 秒读取节点缓存，离开页面后停止页面轮询。进入页面或打开连接详情时，超过 30 分钟（或尚无有效查询时间）的额度会自动刷新；相对时间随页面每分钟的缓存读取更新。`POST /v1/node/profiles/{id}/refresh` 刷新单个连接，遵循节点管理认证及 Mesh 客户端路由限制。同一 Profile 的并发刷新会串行合并，最近 30 秒已有结果时复用快照；修改凭据清空快照后可以立即重新查询。不同 Profile 的刷新互不持有同一把请求锁。

查询失败也保存失败快照；成功刷新 OAuth 凭据后，后续额度查询失败不会丢弃已经刷新好的凭据。手动刷新期间的旧列表响应不会覆盖新额度。

## 验证入口

- `cargo test --locked -p zork-profile -p zork-client-core --lib`
- `cargo test --locked -p zork-agent --lib profiles::tests`
- `cargo test --locked -p zork-station`
- `cargo test --locked -p zork-gui --features headless-bench --test headless_profile_quota --test headless_modals`
- `scripts/test-gui-contracts.py`：新构建的真实节点、认证、公开快照和凭据隔离。
- `scripts/test-client-mesh.py`：已配对客户端通过实际 Mesh 传输刷新连接。
- `scripts/test-desktop-headless.py` 已纳入额度交互和渲染检查。

额度渲染测试覆盖紧凑/宽窗口、无数据、查询失败、零余额、额度耗尽、多周期、中英文、Esc、真实 HTTP 刷新失败后恢复、重复点击合并，以及静止状态不持续重绘和 p95 CPU 绘制预算。供应商返回值使用固定 fixture，不将它当作真实账号接口验收。

## 协议参考

- [OpenAI Codex account/rateLimits/read 文档](https://learn.chatgpt.com/docs/app-server)
- [OpenAI Codex usage 请求实现](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs)
- [OpenAI Codex 额度窗口归一化](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client.rs)
- [Anthropic Claude Code usage 接口问题记录](https://github.com/anthropics/claude-code/issues/31637)

## 连接名称与 token 数量

连接详情支持修改显示名称（1–100 个字符）。名称单独保存，`profile_id`、凭据、模型配置、任务与队员引用均保持稳定；重命名不清空额度快照。节点接口为 `PUT /v1/node/profiles/{id}/name`，请求体 `{ "name": "主力连接" }`。

上下文与输出上限以十进制 K / M 显示，例如 32K、128K、1M。编辑字段支持同样的单位并保留精确 token 数，不把 4096 改成 4000。

## 更新模型与启停

“更新模型”调用 `POST /v1/node/profiles/{id}/models/refresh`，将供应商返回的模型写入连接，并显示新增、补全或已是最新。请求使用连接已有的 API Key 或订阅认证；不支持模型目录的接口会明确报错，失败保留原配置。旧的 `GET .../discovered-models` 仍只读，不保存配置。

更新保留已有模型的手动参数和启停状态，不删除供应商本次未返回的模型。新模型有完整上下文和输出上限时可启用；缺失时显示待配置并保持关闭，不编造上限。目录最多保存 500 个模型，并提示截断。仅提取模型元数据，不导入远端凭据、地址或请求头。

每个模型右侧使用共享开关，调用 `PUT /v1/node/profiles/{id}/models/enabled`，请求体为 `{ "model_id": "...", "enabled": false }`。旧配置默认启用。关闭后无法创建或切换 Agent 使用该模型，已有 Agent 的下一次模型调用也会拒绝；已发出的流式请求不会被中途取消。重复关闭不写文件、不推进同步游标；变更由现有唯一目录投影协调器发布。

模型列表最多显示四行并虚拟滚动。`headless_profile_quota` 覆盖 500 个模型的往返滚动和每帧构建上限。HTTP 合同测试覆盖更新、重复更新、启停、认证和 Agent 拒绝；`test-sync-idle.py` 覆盖重复启停的游标及通知幂等性；`test-client-mesh.py` 覆盖真实配对传输中的启停和 Agent 拒绝。

模型协议参考：[Codex 模型接口实现](https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/endpoint/models.rs)、[Grok CLI 模型接口实现](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-shell/src/remote/model_source/oai.rs)、[Claude 模型列表](https://platform.claude.com/docs/en/api/models/list)。验证使用本地供应商 fixture，不代表真实账号已验收。

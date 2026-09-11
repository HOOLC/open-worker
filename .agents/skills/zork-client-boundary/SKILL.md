---
name: zork-client-boundary
description: 开发、重构或审查 zork 客户端功能及 Rust core/UI 分离时使用；检查 GPUI、Android/JNI 和 Web 展台的业务操作、状态、网络与平台适配边界，防止业务逻辑回流 UI。
---

# 客户端业务边界

先读 [Rust core/UI 硬边界](../../../docs/client-core-ui-boundary.md)。该文档维护职责与迁移状态；本 skill 负责把规则落实到每次开发、审查和验证。设备与会话的订阅接口见 [client-state-subscriptions](../../../docs/client-state-subscriptions.md)，实际行为以当前源码核对。

修改 core 到 UI 的订阅、快照/增量或高频更新调度时，应用 [zork-client-subscriptions](../zork-client-subscriptions/SKILL.md)，守护已应用版本基线、慢消费者恢复和平台消费边界。

## 不变条件

- UI 只展示、制作动效、采集输入并持有瞬态交互状态。业务校验、默认值、权限、状态归并、网络协议与请求、持久化、同步、重试、取消和操作生命周期都由 `zork-client-core` 维护。
- UI 提交业务意图与输入值，读取 core 的只读快照或变化。core 保持独立于 GPUI、Compose 和视图生命周期；服务端业务权威不因客户端抽取而改变。
- 系统文件选择、剪贴板、窗口和浏览器等平台能力可以通过 core 定义的接口适配。平台适配只实现能力，不决定业务策略，不接管业务连接与状态机。
- 未提交的输入缓冲、焦点、选区、滚动、动画离场元素和派生绘制缓存属于呈现状态；是否合法、能否提交、如何选择默认模型、消息是否送达等由 core 返回。只读快照的呈现映射可以保留，不要将它误判为第二份业务权威。

## 改动前确定整条调用链

对当前任务涉及的入口追踪：用户操作 → core 业务操作 → 服务/存储与状态发布 → 平台订阅 → 展示。读到真实实现，不因函数名含 `core` 或文件位于 Rust 目录就认定合规。

- 找出每份业务数据的权威、持久化位置、控制器与订阅者。同一设备/会话应复用现有 core 控制器，不能为页面新建另一套归并或重连流程。
- 找出每个异步操作的所有者、设备/会话标识、并发与取消语义，以及失败后的权威状态。组件关闭不能替代业务取消协议；旧操作结果不能覆盖另一个设备或会话的数据。
- 检查桌面、Android/JNI、Web 内存适配和相关 fixtures 是否共享同一业务契约。能力暂未暴露时扩充 core，不能在页面里临时补一段业务流程。
- 仅按用户本次范围修改。其他既有违规记录到边界文档的迁移状态；它们不构成新代码绕过规则的理由，也不自动授权全仓重构、安装或发布。

## 实现与审查的判断点

- core 接口表达保存模型、删除模型、连接设备、发送消息等业务操作；视图不构造 HTTP 方法、路径和业务请求体。`Request(method, path, body)` 一类转发包装不能充当页面业务接口。
- core 提供字段校验、可执行动作、业务候选项及结构化结果。UI 根据结果展示错误和禁用状态，不分别实现桌面/Android 的规则或解析错误文案决定重试。
- 操作后从共享订阅接收业务结果。点击回调不手工增删另一份模型、设备、消息或 outbox 列表；outbox 消失不代表已送达，断线不代表未执行。
- 配置编辑在 core 中基于权威状态执行，并沿现有服务端协议处理冲突。将旧 UI 列表整包写回、或移动到 core 后仍无条件覆盖新状态，都不算解决并发编辑。
- 发送/停止条件和取消权限通过 core 能力投影获取，不由 UI 根据任务类型、状态字符串或附件数量另建规则。持久化草稿与偏好、文件导入导出策略、邀请和登录轮询、超时/退避与协议游标均留在 core。UI 的动效时钟、可见性及绘制缓存失效仍属于呈现职责。
- 共享组件只依赖展示所需的类型与资源；core 不反向依赖 UI。Web 通过公开业务契约连接 core 的内存实现，不通过跨 crate `#[path]` 编译 core 私有源码，也不在展台复制业务规则。

## 边界复查

先运行 `python3 scripts/check-client-boundary.py`。它检查 UI 的生产依赖、视图中的 IO/Station 调用、Android 旧请求入口及 Web 对 core 私有源码的引用；修改检查器时同时运行 `--self-test`。CI 执行同一入口。该检查只证明已列出的机械约束，下面的业务调用链复查仍必需。

先确定本次文件与调用者，包括暂存、未暂存和新增文件。以下命令在仓库根目录执行，辅助定位当前入口；路径变化时先用 `rg --files` 找到新位置。

```sh
git diff --name-only
git diff --cached --name-only
git ls-files --others --exclude-standard
rg -n 'node_request|reqwest|/v1/|settingsRequest|actions\.request|repo\.command\("request"' crates/zork-gui/src crates/zork-gui-web/src apps/android/app/src/main/java
rg -n 'std::fs|std::net|store\.(put|save|remove)|acknowledgedMessages|retry_after|while.*isActive' crates/zork-gui/src apps/android/app/src/main/java
```

扫描仅提供线索：检查命中的调用链、通用包装器背后的代码、定时器与业务集合写入，以及相关 `Cargo.toml` 依赖。测试 fixtures、平台画面/系统能力适配和呈现缓存需按实际职责区分；不能凭关键词无命中就宣布合规，也不能用重命名、移文件或宽泛白名单隐藏违规。

## 验证与维护

- 按 [zork-validation](../zork-validation/SKILL.md) 读取当前测试入口并验证受影响代码。网络/持久化流程优先在 core 使用隔离 fixtures 验证，再验证平台命令与投影映射，不为 skill 的措辞编写镜像测试。
- 按实际改动选择行为用例：撤回与回执交错、重复/合并通知、慢订阅者、跨设备或会话切换、并发编辑、撤权、取消与重连。应检查最终业务状态一致，而不是仅检查调用次数或界面能打开。
- 改动同步、游标或重连时使用 [zork-sync](../zork-sync/SKILL.md)；改变呈现或动效时使用 [zork-ui-parity](../zork-ui-parity/SKILL.md)。纯视觉任务不因此附加网络或设备发布验证。
- `scripts/storybook/test_package.py` 只守护共享视觉组件的部分边界，不能替代应用视图与 Android 的调用链审查。新增自动守护应覆盖实际违规入口，并说明静态检查无法证明的范围。
- 整改完成后更新边界文档中对应的迁移项，保留事实和验证依据。交付说明本次迁入 core 的职责、UI 保留的呈现状态、实际验证与尚存问题；不把文档、skill 或静态扫描通过说成代码已经全部整改。

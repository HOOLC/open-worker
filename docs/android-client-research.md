# 安卓客户端前期研究

研究日期：2026-09-07。依据 mini1 当前工作树（包含大量既有未提交改动），不是仅依据 main 已提交版本。本文保留初期研究方案；随后已建立 Android 工程并完成 arm64 交叉编译及模拟器连接验证。当前实现范围、构建和测试以 [Android README](../apps/android/README.md) 为准，尚未进行物理手机验证。

## 建议

首版定位为已有 Zork 设备的移动访问端：手机发消息、查看任务、接收结果、验收结果，Gateway/Agent 继续在原设备执行。保持 Device → Leader → Task 的对象关系，手机界面采用逐级导航。

默认建议 Kotlin + Jetpack Compose 界面，抽取 Rust 客户端核心，通过 UniFFI/JNI 接入。Zork 最新工作树已完成进程内嵌入 Synch，下一步验证现有嵌入实现的安卓兼容性和连接，再全面制作页面。GPUI Mobile 值得一个有明确退出条件的兼容性实验；目前不能把现有桌面组件直接复用当作已证实能力。

## 已核实的项目基础

| 能力 | 当前证据 | 安卓含义 |
| --- | --- | --- |
| 原生组件 | `crates/zork-ui` 使用 `gpui-unofficial =1.17.0-pre` | 品牌资源、设计参数可沿用；Compose 需要实现对应控件 |
| Web | `crates/zork-gui-web/README.md`、`src/api.rs` | WASM 组件/设置演示使用内存适配器，不是完整联网客户端 |
| 通信 | `crates/zork-gui/src/api.rs` 的 `GatewayClient` | HTTP/SSE 与 Mesh 请求、订阅已有适配；可作为共享核心抽取起点 |
| Mesh 生命周期 | `crates/zork-gui/src/desktop/transport.rs` | 最新实现由 GUI 进程内 Tokio runtime 承载 Synch，无独立传输辅助进程 |
| Synch 控制 | `crates/zork-mesh/src/managed.rs`、`node.rs` | 直接 `Node::open`，通过 `MeshNode` 调用 engine；已移除 `synch-cli` 依赖及本地 gRPC 控制层，固定 Synch 0.1.8 revision |
| 客户端数据 | `crates/zork-gui/src/desktop/store.rs` | SQLite 保存设备、缓存、文件、outbox；草稿清空与入队同一事务，可抽出复用 |
| 去重 | `crates/gateway/src/http.rs` | 消息携带稳定 `request_id`，服务端核对回执；不能推导所有任务操作都具有相同幂等性 |
| 权限 | `crates/gateway/src/mesh.rs` | 独立 client 授权、路由白名单和订阅撤权检查；手机应保留客户端身份 |
| 首次接入 | `desktop/mod.rs::sync_mesh_devices`、`docs/mesh-onboarding.md` | 现有注册依赖已授权入口；现有安装邀请面向 Gateway，手机首次配对仍需单独设计 |

特别注意两条产品语义：可见回复只来自显式 `chat.post_message`，不能直接显示内部 Agent transcript；运行结束不等于产品任务验收完成。

## 技术路线比较

| 路线 | 复用与成本 | 判断 |
| --- | --- | --- |
| Compose + Rust 核心 | 重做移动 UI；复用协议、投递、缓存规则，接入系统键盘/文件/生命周期 | 推荐基线；仍需证明 Rust Mesh 可在安卓运行 |
| GPUI Mobile + Rust 核心 | 可能复用更多 `zork-ui`，但其 GPUI 来自 Zed 固定 Git revision，当前项目使用另一个发行包 | 保留实验，不视为可直接替换；先检查依赖/API 差异、中文 IME、触控和无障碍 |
| GPUI WASM + WebView | 有组件演示基础，仍需移动输入、完整应用状态及 Mesh 原生桥 | 不能靠现有 Web 演示直接获得完整安卓客户端 |
| Flutter/React Native + Rust | 同样要新 UI 和原生通信桥；当前工作树没有可直接继承的对应产品应用 | 此时没有明显复用优势，暂不增加一套技术栈 |

Compose 是 Android 官方推荐的原生 UI 工具包：[官方说明](https://developer.android.com/compose)。

GPUI Mobile 已提供 Android arm64 示例，但其清单使用 Zed Git revision `5688167d224b5eca54875d49afb8bfd73a07915a`；这与当前 GPUI 发行来源不同，并不等于 API 必然不兼容，必须编译验证。[项目说明](https://github.com/itsbalamurali/gpui-mobile)、[依赖清单](https://github.com/itsbalamurali/gpui-mobile/blob/main/Cargo.toml)。

## 最大技术风险：Mesh 与移动生命周期

用户告知嵌入改动完成后，已重新核对最新源码：`zork-mesh` 固定依赖 Synch Git revision `6d6283f09c32476dc77c09f76a2b2529a42a558d`，`managed::Runtime` 负责进程内任务启动/关闭，桌面 `ClientMesh` 持有 Tokio executor 和该 Runtime。此前“依赖独立 Synch 进程”的调查结论已被这次更新取代。

用户再次告知完成改动后，已核对新的直接嵌入实现：`managed::start_owned` 取得生命周期锁并调用 `Node::open`，自行管理 anti-entropy、publisher、maintenance、scanner、watcher、replicas、checkouts 后台循环及关闭；`MeshNode` 直接提供身份、授权、对象读写、请求和订阅。`control.rs` 已移除，Cargo 清单和锁文件未再出现 `synch-cli`。旧本地控制 socket/token 只作为迁移残留清理，不再作为当前调用通路。

下一步复用 `MeshNode` 向安卓 UI 暴露有界接口。仍需检查 `synch-cc`、`synch-sock` 等依赖对 Android 的影响，按需拆分仅供客户端使用的 feature，并评估手机所需的后台循环和资源预算。Android 侧不承接 Gateway/Agent 进程管理。上述更新经过源码核对，本次未重跑嵌入测试或进行 Android 编译。

进一步核对 v0.1.8：已有 `Node::init(data_dir, domain)`、`Node::open(NodeConfig)`、`Node::shutdown()`，以及 `trust_add`、`remember_peer` 等接口。`NodeConfig.socket_workers = 0` 明确关闭本机 socket 执行池和对应服务通告，适合作为移动访问端配置；仍可调用远端 `connect_socket`。当前 Zork `managed::start_owned` 尚未显式设置该字段，客户端与执行节点的配置需区分。待验证的是安卓依赖兼容性与 Zork 适配，并非必须等待上游开发嵌入 API。Android 应显式传入应用私有数据目录，不依赖桌面默认目录推断。

Synch 0.1.8 已有 `synch-engine`，提供 `Node::connect_socket`；`synch-sock` 把 eBPF 执行依赖限制在部分桌面系统，连接端不执行远端程序。因此，手机作为连接端有研究价值，无需先移植远端 eBPF 执行环境。**这只证明架构方向，尚不能证明整个依赖图能够在 Android 编译或稳定运行。** 需要检查存储、文件监听、TLS、DNS、网络切换和运行时关闭。[固定版本源码](https://github.com/AFK-surf/synchronicity/tree/v0.1.8)、[平台条件](https://github.com/AFK-surf/synchronicity/blob/v0.1.8/crates/synch-sock/Cargo.toml)。

Iroh 提供 Kotlin/Android 构建入口，但 Iroh 只是下层连接能力，不能直接替代 Synch 协议、身份授权和对象读取。[Iroh Kotlin 文档](https://github.com/n0-computer/iroh-ffi/blob/main/README.kotlin.md)。

前台订阅实时事件；退后台前持久化草稿/待发送状态；回前台重新连接并补拉权威历史。WorkManager 可承担允许延迟的同步工作，不能作为实时常驻连接的承诺。锁屏及时通知要另外解决唤醒与推送，并考虑无 Google 服务设备；第一版明确通知能力边界。[Android 后台任务说明](https://developer.android.com/develop/background-work/background-tasks)。

## 首版范围

- 配对已有设备：手机生成自己的身份，已授权设备确认；邀请过期、单次使用、拒绝和撤销有明确结果。初期可用粘贴配对码，随后加二维码；避免将 Gateway 安装命令直接包装成手机接入。
- 设备 → Leader → 任务列表，显示最后确认状态；设备离线不推断任务停止。
- 创建/继续任务、消息历史、Markdown、活动状态、停止操作；逐条核对 Mesh 白名单支持的实际业务入口。
- 离线已读历史、草稿、待发送队列；进程重启和丢回执后继续使用同一个请求 ID。
- 结果验收/继续修改、产物预览与保存；复用现有大小和完整性校验。
- 设置先提供连接、语言和设备信息。完整模型管理、语音、任意本地文件上传、手机执行 Agent 放到后续范围。

密钥采用 Android Keystore 管理包装密钥、加密保存协议所需私钥材料；原生签名算法是否能直接使用 Keystore 要独立验证。卸载后身份丢失和重新配对的行为需明确，设备身份不与普通 UI 偏好一起无条件备份恢复。

## 实施顺序与验收

1. **连接实验**：Android arm64 加载 Rust 库，生成持久身份，经授权连接隔离测试 Gateway，读设备信息和一页消息，订阅事件。覆盖同网直连及 relay 场景，手机进程结束不影响远端任务。
2. **恢复实验**：发送后断网、丢回执、杀进程、重新打开、Wi-Fi/蜂窝切换；消息无重复可见副本，离线缓存可读，撤销权限后请求被拒绝。
3. **抽取共享核心**：建议新建 `crates/zork-client-core`，包含 DTO、投递/回执、缓存与 transport trait；桌面和 Android 尽量共享现有嵌入传输，平台分别管理生命周期。UI、窗口、文件选择、密钥存储为平台适配。复用现有 outbox/client-mesh 回归场景。
4. **移动 UI**：完成首版任务闭环；中文输入、多行编辑、键盘遮挡、系统返回、长消息滚动、字体缩放、TalkBack、旋转恢复做真机验收。
5. **安装包**：固定 JDK/SDK/NDK/Gradle/AGP 和 Rust 依赖，先 arm64 APK；验证签名、升级保留数据、native 库 16 KB 页兼容，再扩展发布渠道。[Android 16 KB 指南](https://developer.android.com/guide/practices/page-sizes)。

若实验表明 Synch 安卓嵌入需要较大上游改造，再评估经过独立客户端认证的 HTTPS 接入层。它涉及远程入口部署和权限设计，不能仅把现有 loopback runtime API 暴露出去；也不应在未确认前替换 Mesh 产品方向。

## mini1 环境与本次验证范围

本次检查未在 PATH/常规安装位置找到 adb、sdkmanager、Android Studio 或 Android SDK；`java_home -V` 报告无 Java Runtime。磁盘当时可用约 8.4 GiB，低于迁移记录的约 23 GiB。正式构建前需要安排工具链和缓存空间，优先真机，暂不叠加大型模拟器镜像。

只进行了源码、固定版本依赖和官方资料核对；未安装工具链、编译 APK、修改生产代码、启动真实设备任务或生成真实邀请。下一步最有价值的交付是一个能连接隔离 Gateway、支持断线恢复的 Android 实验 APK。

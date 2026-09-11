# 手机通知方案研究

研究日期：2026-09-10。目标是国内 Android，包含没有 Google 服务的手机；同时评估 FCM 对当前私人安装版的适用性。本文件是方案建议，没有接入或部署推送服务，也没有测量真实手机到达率。

建议采用一套 Zork 通知事件与网关，下面连接多种投递通道。有正常 GMS 且网络可达的设备使用 FCM；无 GMS 的国内设备以厂商系统通道为正式路线。私人安装版若暂不具备厂商接入条件，可用 UnifiedPush 做内测补充。同一安装实例选择一个活动通道，避免多路同时发送。

**Google FCM 值得先做，但无法覆盖所有目标手机。**它本身免费，应用不必通过 Google Play 分发；客户端仍要求相应 Google 环境。我们的判断是：具备 GMS 的私人测试手机可以优先验证 FCM，接入门槛比办理多个厂商渠道低。GMS 存在不等于网络、Token 注册和锁屏接收都正常，需要分别测量。[FCM 接入要求](https://firebase.google.com/docs/cloud-messaging/android/get-started)、[Firebase 价格](https://firebase.google.com/pricing)、[FCM 网络要求](https://firebase.google.com/docs/cloud-messaging/network-configuration)。

| 路线 | 适用范围 | 主要代价 | 建议 |
| --- | --- | --- | --- |
| FCM | GMS 可用的 Android | 仍需验证网络与后台接收 | 私人测试优先候选；保留为正式通道 |
| 厂商通道，经一个聚合 SDK 接入 | 主流国产 Android、无 GMS | 厂商资质、分类申请、SDK 与平台维护 | 面向普通用户的国内版主路线 |
| 分别直连各厂商 SDK/API | 同上 | 自己维护多套注册、错误码、配额与点击行为 | 如明确不接受聚合服务，再选择 |
| UnifiedPush | 愿意配置的无 GMS 用户 | 额外安装 distributor，处理后台与省电设置 | 私人/开源内测的可选方案 |
| 自己常驻 Mesh、前台服务或定时轮询 | 进程仍有执行机会的场景 | 常驻提示、资源占用及后台限制 | 不作为默认锁屏推送方案 |

厂商 SDK 的主要价值是复用系统维护的连接。例如小米、vivo 的官方说明都明确这一点。聚合服务可减少多厂商适配工作；候选可先评估个推整合版，以极光作对照，但本文没有证据证明哪家实际到达率更高，也没有取得商业报价。[小米产品说明](https://dev.mi.com/xiaomihyperos/documentation/detail?pId=1533)、[vivo 产品说明](https://developers.vivo.com/doc/d/6b683b474cf64fdab1a0738035c8868e)、[个推厂商整合能力](https://docs.getui.com/getui/start/advance/)。

**首先确认准入，避免写完 SDK 才发现不能用。**小米官方接入流程包含企业认证和应用上架；极光当前接入指南也列出多个厂商的企业/上架要求。聚合 SDK 并不替代这些申请。个推还提示，OPPO/vivo 非官方商店下载应用的公信消息有渠道限制。因此，私人侧载 APK 不能直接等同于正式分发版的推送覆盖能力。厂商控制台的实际开通结果是最终依据。[小米接入流程](https://dev.mi.com/xiaomihyperos/ability/mipush)、[极光参数申请指南](https://docs.jiguang.cn/jpush/client/Android/android_3rd_param)、[个推快速接入指南](https://docs.getui.com/getui/start/accessGuide/)。

小米 2026 规则将 AI 互动列入公信，并在好友聊天场景中排除 AI 发起的聊天。Zork 需要按真实业务分别申请分类；任务状态能否获批为重要通知仍需确认，不能把所有 AI 回复都当聊天私信。[小米 2026 消息分类](https://dev.mi.com/xiaomihyperos/documentation/detail?pId=2321)。

UnifiedPush 允许用户选用、自托管推送服务，但依赖手机上的 distributor。官方入门流程包含安装 distributor 和调整其电池优化设置，因此它适合可接受配置成本的用户，不是原厂 ROM 系统推送能力的直接替代。[UnifiedPush 机制](https://unifiedpush.org/news/20221218_unifiedpush/)、[可选 distributor](https://unifiedpush.org/users/distributors/)。

**建议的消息链路：**

```text
任务所属节点：业务事务 + 持久通知 outbox
                    ↓ HTTPS，授权和重试
             Zork 通知网关
                    ↓ 选择一个活动通道
      FCM / 厂商系统推送 / UnifiedPush
                    ↓
              手机显示通知
                    ↓ 点击
        打开本地会话 → Mesh 补齐内容
```

通知网关保存设备投递地址、授权、通道状态和投递回执，供应商密钥仅留在服务端。任务节点只需出站访问，不要求用户把本地 Station 暴露到公网。现有 `deploy/cloudflare` 只有 iroh relay 和 pkarr discovery，没有推送注册或投递接口；它可以作为部署候选，但不因已有中继就具备手机唤醒能力。是否复用其部署位置，应由国内 Wi-Fi/蜂窝网络实测决定。

默认下发经过渠道审核的状态模板和不可猜测的通知标识。任务正文、代码、结果和密钥继续通过 Mesh 获取，不放入厂商通知。精确路由可以使用受保护的附加数据，点击后还要向权威节点验证事件和访问权限。平台仍会看到投递目标、时间与用于展示的状态信息，不能把这条路径称为完全无元数据的端到端加密。

**不能把“唤醒后先重连 Mesh”作为显示通知的前提。**厂商通知消息应尽可能由系统直接展示。FCM 可使用 notification + data，让后台系统展示状态通知，点击时携带路由；如选择 data-only 以便本地解密或精确过滤，则接收处理应足够轻量，并即时产生通知。FCM 给处理回调的执行时间很短，不适合启动完整 Mesh、翻页同步后再决定提醒。[FCM 接收行为](https://firebase.google.com/docs/cloud-messaging/android/receive-messages)、[FCM 优先级与处理时间](https://firebase.google.com/docs/cloud-messaging/android-message-priority)。

自动展示的代价是应用进程不一定参与投递。免打扰和通知总开关必须同步到发送端；客户端本地过滤不足以阻止厂商直接显示。离线修改设置时，应保留待同步状态，不能宣称服务器已立即应用。系统通知权限与系统渠道设置仍具有最终控制权。

当前 Android `ClientViewModel.foreground(false)` 会暂停连接；桌面通知则根据在线 catalog 差分建立本地 ledger，并有首次基线与十分钟过期规则。这些不是手机后台通知的可靠事件源。建议将共同的事件类型与分类规则抽成共享 Rust 领域逻辑，由 Station 在业务提交时生成持久事件；手机与桌面复用稳定事件 ID 做去重，平台分别负责呈现。不要直接把整个桌面 ledger 搬到服务端，也不要让 Station 依赖客户端传输实现。

实现至少需要以下部分：

1. **权威事件与 outbox。**只由任务所属节点生成回复、待验收、需要处理事件；执行节点镜像不重复发。记录稳定 ID、替换键和过期时间，状态已解决时停止投递旧提醒。
2. **设备注册。**区分安装实例、Mesh 身份和供应商 Token；处理刷新、重装、解绑、撤权及通道切换。绑定基于已有配对授权，中继 allowlist 不等于推送授权。
3. **投递网关。**去重、有界重试、过期丢弃、无效 Token 清理、可用时接入供应商回执。供应商接受请求不等于手机收到，更不等于用户已读。
4. **多端协调。**短时前台阅读状态只能用于优化提醒；在线连接不等于用户正在看。已读与已投递分开，推送和前台 Mesh 事件必须共用 ID。厂商不支持撤回时允许旧通知暂留，点击后呈现最新状态。
5. **Android 适配。**权限、渠道、点击冷启动、去重和缓存恢复；后台收到通知时避免初始化全量客户端服务。

手机设置应保留“接收通知”“会话免打扰”等真正控制应用行为的 Switch。声音和重要程度要读取、尊重 Android 通知渠道，并提供进入系统设置的入口；不要照搬桌面提示音 Switch，却无法改变系统的实际行为。Android 8 起通知渠道拥有声音等行为设置，创建后这些行为主要由用户控制；Android 13 起还需运行时通知授权。[Android 渠道](https://developer.android.com/develop/ui/compose/notifications/channels)、[通知权限](https://developer.android.com/develop/ui/compose/notifications/notification-permission)。

WorkManager 用于有界的补拉与恢复工作，不承担即时推送到达承诺。Doze 会限制后台网络和执行机会；强行停止与普通进程回收必须分开，不能承诺强停后应用仍能执行后台代码。[Doze 与 App Standby](https://developer.android.com/training/monitoring-device-state/doze-standby)、[WorkManager 执行约束](https://developer.android.com/develop/background-work/background-tasks/persistent/getting-started/define-work)、[Android 15 强行停止行为](https://developer.android.com/about/versions/15/behavior-changes-all)。

**落地顺序建议：**先核对目标手机 GMS、分发方式、厂商账号与分类资格；接着在一台手机上验证最小链路。具备 GMS 的私人手机优先跑 FCM；无 GMS 且厂商通道未获准时，用 UnifiedPush 作为明确受限的内测路线。取得厂商通道资格后再接聚合 SDK、扩展机型；共用事件、授权和网关接口，从而保留切换供应商的空间。

验证按 ROM/版本和真实分发方式记录：前台当前会话、前台其他会话、锁屏、系统回收进程、从最近任务划走、用户强停、重启未解锁、断网恢复、Wi-Fi/蜂窝切换、免打扰、Token 更新和重复事件。分别记录网关接受、厂商接受、设备可观测到达/展示、点击及最终同步，测量延迟分布和重复率；不能用一条演示通知推导全机型到达率。

尚未确定的事项是厂商准入实际结果、AI/任务类模板批准范围、GMS 手机的国内网络表现，以及聚合服务的套餐能力与成本。这些需要控制台开通和真机实验，本文没有代替它们给出保证。

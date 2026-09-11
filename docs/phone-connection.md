# 手机扫码连接

桌面端「连接设备」提供「连接手机 / 连接其它设备」两个互斥选项，默认选中手机。
手机页只显示二维码接入；其它设备页只显示 Gateway 加入命令。两种邀请在
切换时各自保留，切换本身不生成新邀请。手机在「连接设备」中选择
「扫一扫连接」，也可以粘贴邀请。桌面显示手机名称并请求确认；点击
「允许连接」后，手机自动保存获准的设备，加载 Leader 和会话。

无需额外云控制面。Gateway 保存邀请和授权，Synch 验证身份并传输，
已有 relay/discovery 配置用于跨网连接。负责成员管理的 Gateway 在首次
授权时需要在线；完成后不需要桌面客户端保持打开。

## 短票据与握手

默认在线票据固定 68 字符：`zj1_`（Gateway）或 `zc1_`（手机），后面是
32 字节接入端点公钥和 16 字节随机凭据的 Base64URL 编码。没有 JSON、
设备名称、成员目录或业务网络配置。现有 discovery 根据公钥查找地址，
TLS 验证接入端点后才发送凭据和获取完整信息。

- 普通在线安装并加入命令为 196 字符；已安装 Zork 时为 83 字符。
- 离线 IPv4 票据为 78 字符；必要时附带地址或私有 discovery URL。
  地址优先遵守显式绑定，其次选择私有局域网地址，避免默认取 VPN 地址。
- Gateway 的未完成邀请只保存在有界内存表中，15 分钟有效、一次使用；
  Gateway 重启使未完成邀请失效。用途在服务端绑定，改前缀不能改变权限。
- 在加密握手内返回设备身份、名称、有效期和后续网络设置；接收方验证
  返回信息与票据匹配，再沿用 Synch 身份证明和桌面确认。
- 手机只能成为 `clients`，不成为 Gateway 或 Worker。桌面确认绑定
  具体手机身份和 claim ID，确认前没有普通 Gateway API 权限。
- 正式授权及提交恢复回执持久保存；相同已授权身份可恢复丢失的成功回执，
  撤销设备后旧邀请不能恢复授权。
- 旧 `zork-mesh1-`、`zork-mesh2-` 和 `zork-client1-` 邀请继续支持。
- 手机端只保留扫一扫与粘贴邀请，手填身份/IP 的接入表单已移除。

## 接口

| 操作 | 接口 |
| --- | --- |
| 生成手机邀请 | `POST /v1/node/mesh/client-invites` |
| 查询扫码/确认状态 | `GET /v1/node/mesh/invites` |
| 确认手机 | `POST /v1/node/mesh/invites/{id}/approve`，包含 `origin`、`claim_id` |
| 取消未完成邀请 | `DELETE /v1/node/mesh/invites/{id}` |
| 撤销已授权手机 | 现有 `POST /v1/node/mesh/members/remove` |

以上管理接口要求现有管理员或已授权客户端访问。手机端共享 core 命令
为 `begin_invitation`、`poll_invitation`、`cancel_invitation`。
旧 Gateway 没有手机邀请接口，需要更新 Gateway 和桌面端后才能出码。

## 验证（2026-09-07）

- 31 项 Rust 单元/回归测试通过（Mesh、core、共享格式、JNI）。
- `scripts/android/test_enrollment.py`：真实隔离 Gateway，验证确认前无授权、
  错误 claim 拒绝、邀请抢占拒绝、进程恢复、重复扫码、撤销、拒绝及输入校验。
- 既有 `scripts/test-mesh-enrollment.py` 也通过：三 Gateway 入网、回执恢复、
  默认 Worker 协作、撤销和移除，原电脑接入流程保留。
- `PhoneEnrollmentTest` 在 Android 16 模拟器和 OPPO PLP110 / Find N6
  均通过：从真实桌面截图解码二维码，经 JNI/Synch 申请，桌面实际按钮确认，
  成功读取 Gateway 并重连。测试使用独立目录和隔离 Gateway，不接入生产节点。
- Find N6 另已验证扫码相机界面：系统授权后打开后置相机，Camera 服务
  状态为 PREVIEW，返回后回到 Zork。未把二维码图片解码和相机预览检查
  冒充手机镜头对着屏幕的完整光学扫码验收。蜂窝网络和公网 relay 强制中继未在此次测试中覆盖。
- 当前用户要求移除所有“交给 Leader”设置/接入入口，已从桌面、Android
  和桌面 story 组件中移除；正常的 Leader 导航及会话保留。

## 短票据交付验证（2026-09-07）

内存邀请重启失效、持久回执恢复、旧格式解码、票据长度与地址选择单测通过。
真实三 Gateway 和手机客户端授权回归通过；Find N6 已完成真实桌面二维码
图片解码、短票据握手、桌面确认、读取与重连。光学扫码与强制公网中继未作为
黑色按钮文字修复通过真机前后截图确认：全局文字样式不再硬编码颜色，按钮
继承自身的内容颜色。最新证据见 `artifacts/android/short-invite/`。

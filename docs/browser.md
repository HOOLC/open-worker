# 页面管理与内嵌浏览器

在对话顶栏点击「页面」，右侧统一管理网页与执行历史。网页使用随 Zork 分发的 CEF/Chromium，不依赖机器安装 Chrome。每个对话保留自己的标签页，收起或最小化侧栏不会关闭页面。界面采用两行布局：标签页与窗口操作在上方，导航与单行地址栏在下方；展开按钮可将浏览器铺满侧栏以外的区域。

地址栏接受网址或搜索词，搜索使用 Google。下载按钮在系统文件管理器中打开 Zork 的下载文件夹；更多菜单提供操作授权、选择元素和接管操作。

在更多菜单中点击「选择元素」后再点击网页，页面 URL、标签页 ID、选择器、文字和经过裁剪的 HTML 会加入当前对话草稿，用户确认发送。选择提示悬浮在底部，不改变网页位置；Esc 退出选择。浏览器页面不接入 zork 的原生 IPC，网页内容不能直接调用 Agent 工具。

「允许 Agent 操作」把当前对话的浏览器交给该对话使用。授权随客户端会话结束，不因 Mesh 配对而自动开启。多个客户端同时提供浏览器时，工具返回设备列表，调用方指定 `device_id`。点击「接管操作」会撤销当前对话的操作授权，并把键盘焦点交回同一个内嵌页面。

从对话成员菜单打开执行历史，会在同一标签栏增加历史页。历史保持 GPUI 原生视图，不显示网址栏；切换回网页不重新加载。两种页面共用面板宽度、展开和收起状态。

## 实现边界

- 页面和登录资料在展示网页的客户端上。执行节点可以在其他设备；浏览器请求沿现有的客户端 HTTP / Mesh 连接往返，无需让客户端运行 Gateway 或 Agent。
- `zork-browser-runtime` 在独立进程承载 CEF。GPUI 显示 CEF 直接绘制的 BGRA 页面帧，支持显示缩放，并转发键鼠及输入法事件；不经过 Chrome 截屏流或 JPEG 压缩。输出只保留最新待显示画面，侧栏隐藏时停止绘制和画面传输。
- CEF 使用固定的独立资料目录 `client/browser/cef`，下载位于该目录的 `downloads`；不读取或复制用户其他浏览器的 Cookie。桌面直接启动并持有浏览器子进程，macOS 继承桌面的图形安全会话，使用正常钥匙串加密并在退出前刷写 Cookie。浏览器启动不再调用 `open -a`，也不依赖子应用的 Launch Services 注册查询。
- 控制请求通过私有本地 IPC 到 CEF API，不开启浏览器 TCP 调试端口。保留默认 User-Agent、浏览器属性、沙箱和网页安全检查；不使用属性伪装或 mock keychain。
- 本项对 Google 的要求只涵盖 zork 实现额外引入的浏览器特征。没有加入针对验证码的限频、重试次数限制、识别或自动处理流程。
- 当前打包和直接运行验证平台为 macOS；其他平台仍需补齐分发与原生运行验证。完整安装包包含浏览器 framework、runtime 和 renderer helpers。
- macOS 中浏览器仍位于主应用的 `Contents/Helpers/ZorkBrowser.app`，保留 bundle 身份、签名、CEF 资源和沙箱 helpers。运行时显式指定自己的 `main_bundle_path`、`framework_dir_path` 和 `browser_subprocess_path`，避免 CEF 按外层主应用寻找资源。独立组件能启动不能替代完整安装包验证。
- 父子进程通过继承的 stdin/stdout 管道通信；启动握手失败或超时后，桌面先终止并回收真实浏览器进程再返回错误。错误保留退出状态和本次启动的 stderr，不混入历史日志。父进程退出或关闭输入管道时，浏览器正常关闭并刷写资料，退出卡住时十秒内终止自身。
- 每次启动有独立代次，旧事件读取线程迟到的退出和画面通知不能修改新进程的页面状态。运行时保留旧 `--ipc` 入口用于兼容旧版控制器，新客户端只使用继承管道。
- 系统浏览器的登录态不会自动共享。Google OAuth 的嵌入式浏览器限制与网站验证码分别验收，不能承诺更换内核后全部网页登录均被接受。

## Agent 接口

工具名为 `browser`。对话身份由运行时提供，模型不能传入别的对话 ID。

```json
{"request_id":"browser-list-1","action":{"op":"list"}}
```

```json
{"request_id":"browser-open-1","action":{"op":"open","url":"https://example.com"}}
```

后续操作使用返回的 `tab_id`：`navigate`、`back`、`forward`、`reload`、`stop`、`close`、`read`、`click`、`type`、`key`、`scroll` 和 `screenshot`。选择器操作要求精确匹配一个元素。`type` 插入文字；覆盖已有输入时，可先发送 `key` 的 `Meta+A`（macOS）或 `Control+A`。截图由工具保存到执行会话的工作目录，返回 `file_path`，可以通过既有文件交付工具发送。

同一操作重试必须复用原 `request_id` 和参数。Gateway 在 `state/browser.sqlite` 保存请求和回执，重复请求返回已知结果；重启时仍未取得回执的操作不会自动重发。客户端在当前授权连接内也保存执行回执，避免网络重投造成重复点击。超时是状态不确定，不能解释为未执行。

指令通过 `/browser/events` 的 POST SSE 或同一 Mesh 订阅推送，结果通过 `/browser/receipts`
提交。空闲连接只发送缓存心跳，不再每 5 秒重新领指令。连接有 generation，旧连接关闭不能
解除新连接；授权身份与接管后的撤销记录持久化，迟到回执不会恢复授权或覆盖已有结果。
旧 `/browser/poll` 只接受回执或断开，注册请求会要求更新客户端。新协议需要客户端和节点共同升级。
浏览器事件在 core 按 host 发布，GPUI 合并到平台帧再读取标签和最新图像；侧栏没有 16ms 状态轮询。

## 验证入口

先按 `docs/rust-build-cache.md` 加载构建环境。

```sh
cargo build --locked -p zork-gui -p zork-browser-runtime -p zork-browser -p zork -p zork-gateway -p zork-agent-server -p zork-gh --bins --example probe
python3 scripts/package-macos-client.py
export ZORK_BROWSER_RUNTIME="$PWD/.tmp/macos-app.noindex/Zork.app/Contents/Helpers/ZorkBrowser.app/Contents/MacOS/ZorkBrowser"
python3 scripts/lib/gui-test-session.py -- python3 scripts/test-browser-runtime.py --runtime "$ZORK_BROWSER_RUNTIME" --output artifacts/browser/runtime-validation
python3 scripts/lib/gui-test-session.py -- /usr/bin/env ZORK_BROWSER_RUNTIME="$ZORK_BROWSER_RUNTIME" "$CARGO_TARGET_DIR/debug/examples/probe"
cargo test --locked -p zork-gateway browser::tests
python3 scripts/lib/gui-test-session.py --timeout 600 -- /usr/bin/env ZORK_BROWSER_RUNTIME="$ZORK_BROWSER_RUNTIME" python3 crates/zork-gui/tests/test_browser_desktop.py
python3 scripts/test-client-mesh.py
```

`ZORK_BROWSER_RUNTIME` 仅用于未打包的开发测试，完整应用不需要配置。`CARGO_TARGET_DIR` 由构建环境加载入口设置。CEF 构建还需要 CMake 和 Ninja。浏览器原生进程不属于后端 `test:rust` 的测试范围，使用上述专门入口验证。

macOS 客户端在打开资料目录前检查 `SessionGetInfo` 的 `sessionHasGraphicAccess`；SSH 等非图形安全会话会明确失败，避免在无法取得钥匙串加密密钥时继续访问登录资料。此检查不代表钥匙串已解锁或用户已授权。`gui-test-session.py` 只把测试控制器放进登录用户的图形会话，浏览器仍使用生产的直接子进程链路；测试结束注销并删除临时控制器，保留主应用和用户资料。图形终端里也可以直接运行测试控制器。

macOS 打包优先使用本机可用的 Developer ID 或 Apple Development 签名，也可通过 `ZORK_CODESIGN_IDENTITY` 指定身份；未配置证书时使用临时签名；已有证书被吊销时会停止打包，需先更新有效证书。首次访问登录钥匙串时可能出现系统授权弹窗。开发时应保持签名身份稳定，临时签名随重建变化可能再次触发授权。密码仅在系统弹窗输入。桌面断开控制连接后，浏览器先正常关闭；若 CEF 退出卡住，则在十秒后终止自身。

`probe` 使用独立资料目录和本地测试网页，验证浏览器标识、Cookie 重启保留、导航、输入、元素提取、截图及对话隔离。原生 UI 测试需要先重建 GUI、Gateway、zork 和 zork-gh；它使用独立节点与客户端数据。

对已打包的应用，将 `--runtime` 指向 `Zork.app/Contents/Helpers/ZorkBrowser.app/Contents/MacOS/ZorkBrowser` 运行上述浏览器测试。`--startup-only` 可单独验证嵌套应用启动、renderer 执行、画面输出及控制管道关闭后的进程退出，不打开外部网站；它不代表 Cookie、网站登录或完整导航验收。

`test_browser_desktop.py` 使用当前桌面入口，通过设备侧栏连接独立节点，进入领队对话，验证真实 CEF 画面与历史页共享标签栏、中文输入、点击、元素提问、授权及撤销、重复请求和侧栏恢复。控制请求经过真实 Mesh，但节点都在同一台机器，模型使用 fixture；截图保存在 `artifacts/browser/desktop`。`test_browser.py` 提供共用浏览器断言，`native_automation.py` 提供自动化助手；唯一可执行浏览器 UI 测试入口为 `test_browser_desktop.py`。`test-client-mesh.py` 的浏览器响应端是确定性 fixture；这些检查均不能称为真实跨设备全流程验收。

原生帧耗时验收先用 `frame-profiler` feature 重建 GUI，设置 `ZORK_BROWSER_TEST_PROFILE=1`，
并保持显示会话解锁。锁屏或没有平台帧源时，离屏交互测试可显式设置
`ZORK_GUI_TEST_DRIVE_FRAMES=1`，让自动化读取前通过 GPUI 的测试接口交付一帧回调；
它不修改业务订阅，也不启动常驻刷新。这个模式不能用于原生显示性能验收。

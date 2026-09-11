# Rust 构建缓存约束

## 目标

workspace 内直接依赖同一个第三方 crate 时，使用同一版本和同一组 feature，避免单包和多包构建仅因 member 的声明不同而生成额外编译变体。

## 约束

- feature 曾经存在分叉的公共依赖在根 `Cargo.toml` 的 `[workspace.dependencies]` 中声明一次，取 workspace 当前实际需要的 feature 并集。
- member 通过 `dependency.workspace = true` 继承，不再声明自己的版本、默认 feature 或 feature 子集。
- 不添加仅用于影响 feature 解析的依赖或 crate。
- 不把 debug/release、目标平台、编译器版本、源代码和 `rustflags` 不同的产物视为同一缓存项。

当前统一的直接依赖是 `reqwest`、`tokio`、`futures-util`、`serde`、`tokio-tungstenite`、`ulid` 和 `getrandom`。`getrandom` 的 `wasm_js` 后端仅在 Web crate 的 wasm 目标依赖中启用。

## 边界

稳定 Cargo 根据一次命令实际选中的包统一 feature。`[workspace.dependencies]` 统一直接依赖声明，但不会强制没有直接使用某个依赖的 member 激活该依赖，也不会改写传递依赖的 feature。

## 验证

`test/workspace-cargo-features.test.ts` 检查上述依赖只有一份 workspace 声明，并且 member 的直接使用全部继承该声明。

## 可选的本机存储配置

真实机器路径放在被 Git 忽略的 `.env`，不写进共享脚本或 skill：

```dotenv
ZORK_BUILD_ROOT=/path/to/dedicated-build-cache
ZORK_BUILD_BUDGET_GIB=100
ZORK_BUILD_LOW_WATER_GIB=80
# 仅网络盘需要；未挂载则拒绝运行。
# ZORK_BUILD_MOUNT=/path/to/mount
```

`ZORK_BUILD_ROOT` 下默认 Cargo 输出为 `target`，Android 独立入口为
`android`；手工隔离构建可放 `isolated/<任务名>`。不配置时仍使用项目
`target`，不会要求共享盘。路径相对仓库根目录解析，支持引号和 `~`，
不执行命令、也不展开 `$变量`。进程环境变量优先于 `.env`；显式
`CARGO_TARGET_DIR` 优先于自动生成的路径。

现有 pnpm 的 Rust build/dev/test/start 入口，以及 Android、storybook、
桌面 headless Python 入口读取配置。其他脚本或直接 Cargo 命令不会自动
读取 `.env`，在仓库根目录先执行：

```sh
eval "$(python3 scripts/lib/build_env.py --shell)"
cargo test --locked -p zork-config
```

以上只输出允许的构建变量，不会加载产品密钥。切换仓库或修改 `.env`
后建议开新 shell；已经 export 的值会优先，必要时先 unset 再加载。

### 预算与显式回收

```sh
pnpm cache:status          # 只预览
pnpm cache:prune           # 显式执行回收
```

预算统计配置根目录的磁盘占用。超出高水位后，按最后修改时间选择旧
`target`、`android` 和 `isolated/*` 中带 Rust 缓存标识的目录，目标降到
低水位。默认保留最近 24 小时修改的目录；不遍历任意源码目录，不跟随
候选目录软链接。根目录内的其他文件会计入占用，但不会自动删除。

**执行回收前停止这个缓存根目录的构建和运行任务，并在清理结束前不要
启动新任务。** 脚本再次检查文件修改时间和打开的文件，检查失败或目录
在使用中则拒绝/跳过；这些检查无法对未协作的新进程提供原子互斥。
随后调用 `cargo clean --target-dir`，不直接删除源码或数据库。
共享目录需由同一主机专用；本机无法确认其他主机的打开文件。

这是手动软预算，不是文件系统硬配额；近期或在用产物可能使预算无法
达到。kache 的本地存储预算、共享远端容量与这里的 target 预算互相独立。

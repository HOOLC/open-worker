# ID 与凭据

项目新生成的实体、请求、回执、视图和临时文件 ID 使用 ULID，Rust 依赖统一从 workspace 继承。Android 通过 `NativeBridge.newId()` 调用同一 Rust 实现；Web 使用浏览器随机源。附件的 `file-` 等领域前缀保留，后面的新标识为 ULID。

已有 ID 是不透明字符串，不重写数据库、缓存、outbox 或引用。服务链接和邀请读取同时兼容历史 32 位十六进制 ID。更新前已生成的请求在恢复、查询和重试时继续使用原 ID，不能重新生成。

ULID 含时间戳，不能作为访问凭据。管理、任务、浏览器与 Mesh 凭据使用 32 字节 OS 安全随机数；OAuth PKCE 也直接使用 OS 随机字节。已有凭据继续有效，不在启动时轮换。

以下值不属于 UUID 迁移范围：外部服务返回的 ID、Mesh 公钥、内容哈希、短邀请协议从随机 capability 派生的 ID、Apple 构建 UUID 和第三方库内部的 UUID。Cargo.lock 中保留的 UUID 是传递依赖，不是项目直接依赖。

ULID 的时间字段不是跨设备的权威顺序，同步仍使用现有的 sequence、epoch 和分页游标。

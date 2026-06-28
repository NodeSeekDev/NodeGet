# ng-static：静态文件 Bucket 管理

> 概览：ng-static 负责管理 NodeGet 的静态文件 bucket（数据库表 `static_file` 中 bucket name → 磁盘子目录的映射），通过 HTTP（`/nodeget/static/{name}`）对外提供文件下载，并提供 WebDAV 端点（`/nodeget/static-webdav/{*path}`）和两个 JSON-RPC 命名空间（`static-bucket` 做 bucket CRUD，`static-bucket-file` 做文件 upload/read/delete/rename/list）。所有 bucket 元数据写操作（create/update/delete）触发全表内存缓存（StaticCache，基于 `DbBackedCache` + `make_global_cache!`）刷新；文件 upload/delete/rename 不会 reload StaticCache；所有磁盘路径锚定在可配置的 `static_path`（默认 `./static/`）之下，并通过 `validate_name`/`validate_sub_path`/`resolve_safe_file_path` 三件套防御目录穿越。

## 模块结构

```
crates/ng-static/src/
├── lib.rs                          # Crate root：FileInfo（始终可用）+ server feature 下模块再导出
├── cache.rs                        # 全表缓存 StaticCache（make_global_cache! 单例）
├── ops.rs                          # 核心业务：bucket CRUD、文件 upload/read/delete/rename/list、path 安全校验、static_path 配置缓存
├── router.rs                       # axum Router：静态文件服务 + WebDAV（DavHandler 按 bucket 缓存、ETag、MIME、CORS、nosniff）
├── auth.rs                         # RPC 层权限辅助（委托 PermissionChecker，用于两个 RPC 命名空间）
└── rpc/
    ├── mod.rs                      # 合并 static-bucket + static-bucket-file 的 RpcModule
    ├── static_bucket/
    │   ├── mod.rs                  # static-bucket trait + impl
    │   ├── auth.rs                 # SuperToken 校验辅助（供 list 使用）
    │   ├── create.rs               # create
    │   ├── read.rs                 # read
    │   ├── update.rs               # update
    │   ├── delete.rs               # delete
    │   └── list.rs                 # list（super-token only）
    └── static_bucket_file/
        ├── mod.rs                  # static-bucket-file trait + impl
        ├── auth.rs                 # 占位模块；当前文件级权限校验仍走 crate::auth
        ├── upload_file.rs          # upload
        ├── read_file.rs            # read
        ├── delete_file.rs          # delete
        ├── rename_file.rs          # rename（同时校验 Write + Delete）
        └── list_file.rs            # list
```

| 文件 | 角色 |
|------|------|
| `lib.rs` | 定义始终可用的 `FileInfo`，feature-gate 全部 server-only 模块（auth/cache/ops/router/rpc），并再导出 server 公共 API 面 |
| `cache.rs` | DB-backed 全表缓存 `static_file`，按 bucket name 索引，跟踪唯一的 `is_http_root` bucket |
| `ops.rs` | 核心业务逻辑：bucket CRUD、文件 CRUD、path 安全校验、`static_path` 配置缓存。bucket 元数据写操作会调用 `StaticCache::reload()`；文件写操作不会 |
| `router.rs` | axum Router：`/nodeget/static/{name}[/{*path}]` 静态文件 + `/nodeget/static-webdav/{*path}` WebDAV；DavHandler 按 bucket 缓存；ETag/If-None-Match、MIME 猜测、CORS、bucket 根请求映射到 `index.html`，且缺失路径时额外尝试 `{path}/index.html`、nosniff |
| `auth.rs` | 委托 `PermissionChecker` 的权限辅助（服务于两个 RPC 命名空间） |
| `rpc/mod.rs` | 合并两个命名空间的 `RpcModule`，由 server 二进制合并 |

## 公共 API

### 类型

| 名称 | 签名 | 行为 |
|------|------|------|
| `FileInfo` | `pub struct { pub path: String, pub size: u64, pub mtime: i64 }`（`Serialize+Deserialize+Debug+Clone+PartialEq+Eq`，`crates/ng-static/src/lib.rs:21`）| 始终可用（无 feature gate），由 `static-bucket-file.list` 返回。`path` 使用 `/` 分隔符（跨平台）；`mtime` 为 Unix 毫秒，不可用时为 `0` |

### 函数（server feature）

| 名称 | 签名 | 行为 | 锚点 |
|------|------|------|------|
| `get_static_path` | `pub fn get_static_path() -> String` | 返回配置的 static_path（默认 `./static/`）。首次调用读取 config 并缓存，后续返回缓存值；通过 `reload_static_path()` 热刷新 | `crates/ng-static/src/ops.rs:56` |
| `reload_static_path` | `pub fn reload_static_path()` | 重新从 config 读取并写入 ArcSwap。配置热加载后必须调用 | `crates/ng-static/src/ops.rs:74` |
| `validate_name` | `pub fn validate_name(name: &str) -> anyhow::Result<()>` | 校验 bucket name：非空、≤128 字符、仅 `[A-Za-z0-9_.-]`、非全点（`.`/`..`/`...`）；失败返回 `NodegetError::InvalidInput` | `crates/ng-static/src/ops.rs:84` |
| `validate_sub_path` | `pub fn validate_sub_path(path: &str) -> anyhow::Result<()>` | 校验 bucket 子路径：非空、≤512 字符、无反斜杠、非绝对/ParentDir/Windows Prefix；每个 `Normal` 段递归过 `validate_name`；`CurDir` 跳过；至少需一个 `Normal` 段 | `crates/ng-static/src/ops.rs:114` |
| `resolve_safe_file_path` | `pub fn resolve_safe_file_path(static_path: &str, sub_path: &str, file_path: &str) -> anyhow::Result<PathBuf>` | 安全解析 `{static_path}/{sub_path}/{file_path}`：base = static_path/sub_path（防御性再校验 sub_path）；遍历 file_path 分量：`Normal` push、`RootDir`/`CurDir` 跳过、`ParentDir` pop（pop 失败即逃逸→报错）、`Prefix` 拒绝；最后做词法级 `starts_with(base)` 校验 | `crates/ng-static/src/ops.rs:174` |
| `create_static` | `pub async fn create_static(name: String, path: String, is_http_root: bool, cors: bool) -> anyhow::Result<Model>` | 校验；查重；is_http_root 唯一性检查；insert ActiveModel；mkdir -p `{static_path}/{path}`（失败仅 warn）；`StaticCache::reload` | `crates/ng-static/src/ops.rs:233` |
| `read_static` | `pub async fn read_static(name: &str) -> anyhow::Result<Option<Model>>` | 仅查缓存（无 DB），返回克隆的 model | `crates/ng-static/src/ops.rs:302` |
| `update_static` | `pub async fn update_static(name: String, new_path: String, new_is_http_root: bool, new_cors: bool, new_enable: Option<bool>) -> anyhow::Result<Model>` | 校验；按 name 查行；若开启 is_http_root 则用 `Id.ne` 检查唯一性；更新全部四个字段（enable 使用 `Set(Option<bool>)`）；缺失新目录则创建（不迁移旧内容）；`StaticCache::reload`；`clear_dav_handler_cache` | `crates/ng-static/src/ops.rs:327` |
| `delete_static` | `pub async fn delete_static(name: &str) -> anyhow::Result<()>` | 校验 name；查行；`delete_by_id`；`StaticCache::reload`；`clear_dav_handler_cache`。**不删除磁盘目录** | `crates/ng-static/src/ops.rs:400` |
| `upload_file` | `pub async fn upload_file(name: &str, file_path: &str, body: Option<Vec<u8>>, base64_str: Option<String>) -> anyhow::Result<()>` | body 与 base64 必须二选一（两者都给或都不给→错误）；查缓存；按需 base64 解码；`resolve_safe_file_path`；`create_dir_all(parent)`（warn-only）；`tokio::fs::write` | `crates/ng-static/src/ops.rs:439` |
| `read_file` | `pub async fn read_file(name: &str, file_path: &str) -> anyhow::Result<String>` | 校验；安全解析路径；读文件（NotFound→`NodegetError::NotFound`，其它→IoError）；返回 STANDARD base64 编码 | `crates/ng-static/src/ops.rs:502` |
| `delete_file` | `pub async fn delete_file(name: &str, file_path: &str) -> anyhow::Result<()>` | 校验；安全解析；`remove_file`；NotFound 视为成功（幂等） | `crates/ng-static/src/ops.rs:536` |
| `rename_file` | `pub async fn rename_file(name: &str, from: &str, to: &str) -> anyhow::Result<()>` | 校验；from/to 均锚定同一 bucket 根；路径相等→no-op Ok；`create_dir_all(to parent)`（warn-only）；`tokio::fs::rename`（NotFound→`NotFound("Source file not found")`）。**不支持跨 bucket** | `crates/ng-static/src/ops.rs:597` |
| `list_file` | `pub async fn list_file(name: &str) -> anyhow::Result<Vec<FileInfo>>` | 校验；base = static_path/model.path；`spawn_blocking(collect_files(&base))`；目录缺失→空 vec | `crates/ng-static/src/ops.rs:572` |
| `list_all_names` | `pub async fn list_all_names() -> Vec<String>` | 取 `StaticCache::global().get_all()`，map name，字典序排序 | `crates/ng-static/src/ops.rs:638` |
| `clear_dav_handler_cache` | `pub fn clear_dav_handler_cache()` | 清空所有缓存的 DavHandler；DAV_HANDLER_CACHE 未初始化时为 no-op。`update_static`/`delete_static` 自动调用 | `crates/ng-static/src/router.rs:42` |
| `router` | `pub fn router() -> axum::Router` | 注册 `/nodeget/static/{name}`、`/nodeget/static/{name}/{*path}`、`/nodeget/static-webdav/{*path}` 三条路由 | `crates/ng-static/src/router.rs:82` |
| `rpc_module` | `pub fn rpc_module() -> jsonrpsee::RpcModule<()>` | 合并 `StaticBucketRpcImpl` + `StaticBucketFileRpcImpl` 的 `into_rpc()`；合并失败 `.expect` panic | `crates/ng-static/src/rpc/mod.rs:15` |

## 关键类型与常量

### `StaticCache`（`crates/ng-static/src/cache.rs`）

| 项 | 锚点 | 说明 |
|----|------|------|
| `CachedStatic` | `cache.rs:19` | `pub struct { model: Arc<static_entity::Model> }`；Arc 共享避免克隆 |
| `StaticCacheInner`（私有） | `cache.rs:25` | `by_name: HashMap<String, CachedStatic>` + `http_root_name: Option<String>`（单一 is_http_root bucket 名） |
| `StaticCache` | `cache.rs:35` | `pub struct { inner: RwLock<StaticCacheInner> }`；全表内存缓存 |
| `recover_read`/`recover_write`（私有） | `cache.rs:43` | 通过 `unwrap_or_else(\|e\| e.into_inner())` 获取锁守卫——锁中毒时 warn 并恢复内部数据而非 panic |
| `make_global_cache!(StaticCache, STATIC_CACHE_GLOBAL)` | `cache.rs:63` | 生成 `StaticCache::init()`/`::global()`/`::reload()` OnceLock 单例（global key `STATIC_CACHE_GLOBAL`） |
| `impl DbBackedCache for StaticCache` | `cache.rs:65` | `type Model = static_entity::Model`；`cache_name()="static_file"`；`build_cache()` 构建映射；`reload_from_models()`（async，`#[allow(unused_async)]`）交换内部 map 并显式 drop 写守卫；`load_all()` 返回 `load_from_db::<static_entity::Entity>()` |
| `StaticCache::build_maps` | `cache.rs:106` | `fn(models) -> (HashMap, Option<String>)`：按 name 索引，挑选首个 is_http_root=true 的条目；重复 is_http_root 时保留首个并以 `warn(target:"static")` 忽略其余 |
| `StaticCache::get_by_name` | `cache.rs:139` | `pub fn(&self, name: &str) -> Option<Arc<Model>>`：读锁查找 |
| `StaticCache::get_http_root` | `cache.rs:147` | `pub fn(&self) -> Option<Arc<Model>>`：返回 is_http_root bucket 的 model |
| `StaticCache::get_all` | `cache.rs:154` | `pub fn(&self) -> Vec<Arc<Model>>`：全部缓存条目 |
| `StaticCache::exists` | `cache.rs:163` | `pub fn(&self, name: &str) -> bool`：`contains_key` 检查 |

### `STATIC_PATH` 缓存（`crates/ng-static/src/ops.rs`）

| 项 | 锚点 | 说明 |
|----|------|------|
| `STATIC_PATH` | `ops.rs:29` | `static STATIC_PATH: OnceLock<ArcSwap<String>>`，配合 `static_path_ref()` 初始化为空字符串。缓存配置的 static_path 以避免每请求走全局 config RwLock + String clone |
| `read_static_path_from_config` | `ops.rs:37` | `fn() -> String`：读 server config，未设置或不可读时默认 `./static/` |
| `get_static_path` | `ops.rs:56` | `pub fn() -> String`：返回缓存的 static_path；首次调用惰性从 config 初始化（compare_and_swap 风格——并发初始化无害） |
| `reload_static_path` | `ops.rs:74` | `pub fn()`：重读 config 存入 ArcSwap；**配置热加载后必须调用** |

### `DAV_HANDLER_CACHE`（`crates/ng-static/src/router.rs`）

| 项 | 锚点 | 说明 |
|----|------|------|
| `DAV_HANDLER_CACHE` | `router.rs:32` | `static DAV_HANDLER_CACHE: OnceLock<RwLock<HashMap<String, DavHandler>>>`。DavHandler 按 bucket name 缓存（LocalFs 永久绑定磁盘路径） |
| `clear_dav_handler_cache` | `router.rs:42` | `pub fn()`：清空所有缓存；未初始化时 no-op |
| `get_or_create_dav_handler` | `router.rs:52` | `fn(bucket_name, disk_path) -> DavHandler`：读锁快路径；写锁慢路径双检；用 `LocalFs(disk_path,false,false,false)` + `FakeLs` locksystem + `strip_prefix("/nodeget/static-webdav/{bucket_name}")` 构建；缓存并 clone（DavHandler 内部为 Arc，clone 便宜） |
| `guess_mime_type` | `router.rs:154` | `fn(path: &Path) -> &'static str`：扩展名→MIME 表，回退 `application/octet-stream`。覆盖 html/css/js/json/png/jpg/gif/svg/ico/woff2/woff/ttf/txt/xml/wasm |
| `serve_static_file` | `router.rs:190` | `async fn(sub_path, path, cors, method, if_none_match) -> Response<HttpBody>`：仅 GET/HEAD（其它→405 带 Allow）；`resolve_safe_file_path`；bucket 根请求映射到 `index.html`，若目标缺失则额外尝试 `{path}/index.html`；弱 ETag（`mtime_secs-size`）；If-None-Match 命中→304；设置 Content-Type、ETag、`X-Content-Type-Options:nosniff`、可选 CORS `ACAO:*`；HEAD 不带 body |
| `build_etag` | `router.rs:300` | `fn(metadata) -> String`：`format!("\"{}-{}\"", mtime_secs, size)`；仅用于缓存协商的弱校验器 |
| `if_none_match_is_match` | `router.rs:313` | `fn(Option<&str>, etag) -> bool`：`*` 匹配全部；支持逗号列表；剥离 `W/` 弱前缀 |
| `static_webdav_handler` | `router.rs:333` | `async fn(Request) -> Response`：从 URI 解析 bucket name；查 StaticCache（enable==Some(false)→404）；提取 Basic Auth（base64 解码）；先按 `user:pass` 再按 `user|pass` 尝试 `TokenOrAuth` 解析；一次性检查全部四个 StaticBucketFile 权限（Read/Write/Delete/List）于 `Scope::StaticBucket(name)`；`get_or_create_dav_handler`；`dav.handle(req).into_response()` |
| `build_webdav_auth_required` | `router.rs:481` | `fn() -> Response`：401 带 `WWW-Authenticate: Basic realm="NodeGet Static WebDAV"` |
| `build_webdav_error` | `router.rs:493` | `fn(status, message) -> Response`：401/403 等 WebDAV 错误纯文本 body；无 CORS |
| `build_http_error` | `router.rs:505` | `fn(status, message) -> Response<HttpBody>`：静态文件路由纯文本错误；无 CORS |
| `build_static_error` | `router.rs:520` | `fn(status, message, cors) -> Response<HttpBody>`：可选附加 `ACAO:*` 让浏览器可读错误 body |

### 权限辅助（`crates/ng-static/src/auth.rs`）

| 项 | 锚点 | 说明 |
|----|------|------|
| `check_static_bucket_permission` | `auth.rs:26` | `pub async fn(token, name, permission: StaticBucketPermission) -> anyhow::Result<()>`：`from_full_token` 解析；取 `require_permission_checker`；`check_token_limit(Scope::StaticBucket(name), Permission::StaticBucket(permission))`；失败→`PermissionDenied` |
| `check_static_bucket_file_permission` | `auth.rs:63` | 同形态，使用 `Permission::StaticBucketFile(permission)` |
| `auth::check_super_token` | `rpc/static_bucket/auth.rs:14` | `pub async fn check_super_token(token: &str) -> anyhow::Result<bool>`：解析 token、取 checker、`check_super_token`；解析失败或缺少 checker→Err |

### `collect_files`（`crates/ng-static/src/ops.rs:652`）

`fn(&Path) -> Result<Vec<FileInfo>>`：显式 `VecDeque` 栈（无递归）；缺失或非目录→空；使用 `symlink_metadata` 检测并跳过符号链接（不跟随）；仅列出常规文件；非 UTF8 路径段以 warn 跳过；mtime 毫秒（失败为 0）；输出按 path 排序。

## 内部机制

### Cache reload after writes

所有 bucket 写操作（`create_static` `ops.rs:291`、`update_static` `ops.rs:380`、`delete_static` `ops.rs:415`）结尾均调用 `StaticCache::reload().await?`，这会通过 `DbBackedCache::load_all` 重新读取整张 `static_file` 表并重建内存 map。读取（`read_static`）与文件操作只命中 StaticCache——从不触 DB。

### static_path + DavHandler caching

三套不同的单例模式并存：

- **StaticCache**：使用 `make_global_cache!`（OnceLock + DbBackedCache）。
- **STATIC_PATH**：手写 `OnceLock<ArcSwap<String>>`（`ops.rs:29`），通过 `reload_static_path` 支持热替换。
- **DAV_HANDLER_CACHE**：手写 `OnceLock<RwLock<HashMap>>`（`router.rs:32`）。

**DavHandler 缓存不会在 static_path 配置热加载时自动清空**——只有 `update_static`/`delete_static` 调用 `clear_dav_handler_cache()`。因此仅靠 config 改 static_path 时，调用方必须显式调用 `clear_dav_handler_cache()`（及 `reload_static_path()`），否则缓存中的 DavHandler 仍指向旧磁盘路径，或重启服务。

### Path safety

`resolve_safe_file_path`（`ops.rs:174`）防御性地再校验 sub_path，然后遍历分量：`Normal` push、`RootDir`/`CurDir` 跳过、`ParentDir` pop（若会逃逸 base 则报错）、`Prefix` 拒绝。最后的 `starts_with(base)` 是词法级兜底，用于再次确认结果仍在 base 目录树内；它不做 canonicalize，也不检查符号链接。`list_file` 的 `collect_files`（`ops.rs:652`）使用 `symlink_metadata` 并显式跳过符号链接文件类型，因此列目录时不会跟随 symlink。

### enable flag semantics

HTTP 路由将 `enable==Some(false)` 视为 404（`router.rs:95,126`）；WebDAV 路由与之对齐：`enable==Some(false)`→404（`router.rs:364`，新增以消除「HTTP 404 但 WebDAV 仍开放」的不一致）。`enable==None` 与 `enable==Some(true)` 均正常服务。

### DavHandler double-checked locking

`router.rs:64-66` 双检模式：读锁快路径返回 clone 的 handler；未命中则取写锁、再检查、再构建并缓存。DavHandler clone 便宜（内部 Arc）。锁中毒会通过 `.expect` panic（`router.rs:46,62`）——这点与可从中毒恢复的 StaticCache 不同。

### RPC instrumentation

每个方法开启 `info_span!(target:"static_bucket[_file]", token_key, username, ...)`，运行 `async { rpc_exec!(<inner>().await) }.instrument(span).await`。trait impl 中被 `rpc_exec!` 包裹的模块函数本身返回 `RpcResult<Box<RawValue>>`；`rpc_exec!` 只负责统一记录成功/失败日志（target=`"rpc"`）并原样返回结果。模块函数内部通常再定义 `process_logic` async 块返回 `anyhow::Result<_>`，并在本模块内通过 `anyhow_to_nodeget_error` 映射为 `ErrorObject::owned(code, msg, None::<()>)`。

### WebDAV auth flow

`static_webdav_handler`（`router.rs:333`）手动从 URI 解析 bucket name（避开 axum 多段 Path 提取器的段数不匹配），剥离前缀 `/nodeget/static-webdav/`，在首个 `/` 处分割。鉴权：要求 Basic 头，base64 解码后先按 `user:pass` 尝试 `TokenOrAuth::from_full_token`，失败再回退 `user|pass`。在单次 `check_token_limit` 调用中要求全部四个 StaticBucketFile 权限。

## RPC 方法

### `static-bucket`

| 方法 | 参数 | 所需权限 | 行为 |
|------|------|----------|------|
| `create` | `token, name, path, is_http_root, cors` | `StaticBucket::Write` on `Scope::StaticBucket(name)` | 校验；查重；is_http_root 唯一性；insert DB 行；mkdir -p；reload cache；返回 model |
| `read` | `token, name` | `StaticBucket::Read` | 缓存查询；缺失→NotFound；返回 model |
| `update` | `token, name, path, is_http_root, cors, enable: Option<bool>` | `StaticBucket::Write` | 更新四字段；is_http_root 转换时查唯一性；建新目录（不迁移）；reload；清 DavHandler |
| `delete` | `token, name` | `StaticBucket::Delete` | 删 DB 行（不删磁盘）；reload；清 DavHandler；返回 `{"success":true}` |
| `list` | `token` | **SuperToken only**（`check_super_token`） | 返回排序的 bucket name 列表（来自缓存） |

### `static-bucket-file`

| 方法 | 参数 | 所需权限 | 行为 |
|------|------|----------|------|
| `upload` | `token, name, path, body: Option<Vec<u8>>, base64: Option<String>` | `StaticBucketFile::Write` | body/base64 二选一；写文件；返回 `{"success":true}` |
| `read` | `token, name, path` | `StaticBucketFile::Read` | 返回 base64 编码内容（RawValue 字符串） |
| `delete` | `token, name, path` | `StaticBucketFile::Delete` | 幂等删除；返回 `{"success":true}` |
| `rename` | `token, name, from, to` | **`Write` AND `Delete`**（两次顺序 `check_token_limit`，`rename_file.rs:36`） | rename = 建目标 + 删源；防 Write-only 绕过 Delete；同路径 no-op；返回 `{"success":true}` |
| `list` | `token, name` | `StaticBucketFile::List` | 返回 `Vec<FileInfo>`（path/size/mtime），已排序，排除 symlink |

### 非 RPC：WebDAV

| 方法 | 参数 | 所需权限 | 行为 |
|------|------|----------|------|
| `static_webdav_handler` | HTTP request；bucket name 来自 URI；Basic Auth 凭据 | `StaticBucketFile::{Read,Write,Delete,List}` **全部** | `enable==Some(false)`→404；DavHandler 按 bucket 缓存 |

**鉴权流**：所有 RPC 方法首参 `token: String`，经 `token_identity(&token)` 拆出 `token_key`+`username` 用于 span 字段；先 `from_full_token` 解析，再 `check_token_limit` 校验对应 Scope/Permission。WebDAV 走独立的 Basic Auth 解析路径（见上文 WebDAV auth flow）。

## 数据库实体

### `static_file` 表（通过 `ng_db::entity::static_file` 作为 `static_entity` 消费）

| 列 | 类型与约束 | 备注 |
|----|------------|------|
| `id` | `i64`，primary_key | — |
| `name` | `String` | bucket 名 |
| `path` | `String` | `static_path` 下的磁盘子目录 |
| `is_http_root` | `bool` | `name` 本身有唯一索引；`is_http_root=true` 在 SQLite/PostgreSQL 迁移中还受 partial unique index 约束，应用层查询仍作为前置校验 |
| `cors` | `bool` | 是否对该 bucket 启用 CORS |
| `enable` | `Option<bool>` | 仅 `Some(false)` 视为禁用——HTTP/WebDAV 路由返回 404；`None` 与 `Some(true)` 均正常服务 |

无 SeaORM relations。`name` 由数据库唯一索引约束；`is_http_root=true` 在 SQLite/PostgreSQL 迁移中还由 partial unique index 约束，`ops::create_static`/`update_static` 的查询检查主要作为更早的应用层报错与不支持 partial index 后端（如 MySQL）的最后防线。`cache.build_maps` 对重复 `is_http_root` 的 warn 仍是防御性兜底，而非常态。磁盘目录 `{static_path}/{path}` 在 create/update 时创建，但 delete 时**从不删除**（仅删 DB 行）。

## Crate 内部约定

- **Feature gate**：`default = []`（仅 `FileInfo` 类型，agent 安全）；`server` feature 拉入 `ng-infra/server`、`ng-db/server`、`ng-config/server`、`ng-core/for-server`、`jsonrpsee/server`、`axum`、`dav-server`、`sea-orm` 等。
- **RPC 模式**：`#[rpc(server, namespace = "static-bucket"/"static-bucket-file")]` + `#[method(name = "...")]`；server 二进制通过 `rpc_module()` 合并。**绝不**手工 `register_method`。
- **所有 RPC handler**：首参 `token: String`，返回 `RpcResult<Box<RawValue>>`；impl 块在 `info_span!` 内用 `rpc_exec!(...)` 包装内部 async 块。`token_identity(&token)` 拆 `token_key`+`username` 用于 span 字段。
- **内部 RPC fn**：模块级函数本身返回 `RpcResult<Box<RawValue>>`；其内部通常用 `process_logic` async 块承载 `anyhow::Result<_>` 业务流程，再经 `anyhow_to_nodeget_error` → `ErrorObject::owned(code, msg, None::<()>)` 映射。
- **Logging targets**：`"static"`、`"static_cache"`、`"static_bucket"`、`"static_bucket_file"`、`"webdav"`。注释保持中文。
- **Path-safety 三件套**（`validate_name`/`validate_sub_path`/`resolve_safe_file_path`）是对抗目录穿越的规范防线；任何新的 path 处理代码须遵守同样纪律（CLAUDE.md 约定）。
- **StaticCache** 使用 `make_global_cache!` 宏 → OnceLock 单例 + `init()`/`global()`/`reload()`；标准 `DbBackedCache` + `load_from_db` 模式。
- **Config hot-reload**：配置热加载后必须调用 `reload_static_path()`；`update_static`/`delete_static` 会自动调用 `clear_dav_handler_cache()`，而 server 当前的 config reload 流程也会同时调用二者，避免 stale LocalFs 绑定。
- **Body 类型**：axum 响应体为 `jsonrpsee::server::HttpBody`（与 RPC server 共享），WebDAV handler 用 `axum::body::Body`。
- **DavHandler caching**：手写 `OnceLock<RwLock<HashMap>>` 单例，**非** `make_global_cache!`。

## 注意事项与陷阱

- **`crates/ng-static/src/ops.rs:74`**：配置热加载（static_path 变更）后**必须**调用 `reload_static_path()`。`get_static_path()` 仅在缓存为空时重读 config，否则永远返回旧值。此外 `clear_dav_handler_cache()` 不会在仅 config 变更时被调用——config reload 钩子必须同时调用二者，否则缓存 DavHandler 仍指向旧磁盘路径。
- **`crates/ng-static/src/ops.rs:400`**：`delete_static` 仅删 DB 行，磁盘目录 `{static_path}/{path}` 永不移除；`update_static` 改 `path` 也不迁移旧目录内容。维护者**切勿**假设磁盘状态与 DB 状态一致。
- **`crates/ng-static/src/cache.rs:106`**：`cache.build_maps` 对重复 `is_http_root` 的 warn 是防御性兜底，不是主约束来源。当前迁移在 SQLite/PostgreSQL 上为 `is_http_root=true` 建了 partial unique index，`ops` 层查询检查提供更早的业务错误；只有不支持 partial index 的后端（如 MySQL）才主要依赖应用层约束。
- **`crates/ng-static/src/router.rs:32`**：DavHandler 缓存锁使用 `.expect`（中毒即 panic，`router.rs:46,62`），与可恢复中毒的 StaticCache 不同。持写锁时 panic 会中毒锁并使所有后续 WebDAV 请求崩溃。
- **`crates/ng-static/src/router.rs:198`**：静态文件路由仅服务 GET/HEAD，其它→405。OPTIONS 预检仅在 `model.cors==true` 时由路由 handler 处理（`router.rs:96,128`）——非 CORS bucket 对 OPTIONS 返回 405。CORS 预检路径返回 `Allow-Methods: "GET, HEAD, OPTIONS"`，而 405 路径返回 `Allow: "GET, HEAD, OPTIONS"`；header 名不一致是设计（一个是普通 405，一个是 CORS 预检）。
- **`crates/ng-static/src/router.rs:402`**：WebDAV Basic Auth 先按 `user:pass` 再回退 `user|pass` 解析。因 Basic Auth 在首个 `:` 处分割，token key 或 password 内含 `:` 会错误分割。token 格式为 `key:secret`（或 `username|password`）；回退覆盖 `|` 形式，但用户名部分含 `:` 无法通过 Basic Auth 表达。
- **`crates/ng-static/src/router.rs:419`**：WebDAV 在单次 `check_token_limit` 调用中要求全部四个 StaticBucketFile 权限。RPC 层（`auth.rs`）每次只查一个权限——因此仅有 Read 的 token 仍可调用 `static-bucket-file.read`，但**永远无法**使用 WebDAV。
- **`crates/ng-static/src/ops.rs:562`**：`list_file` 在 `spawn_blocking` 下调用同步的 `collect_files`。它通过 `symlink_metadata` 跳过符号链接；但 `resolve_safe_file_path` **不**作用于 list 结果——`collect_files` 直接遍历磁盘，且 read/write/delete/rename 的 lexical path 校验也不会 canonicalize 或阻止已落盘 symlink 被后续 I/O 跟随。当前文档只能如实描述这一点，不应把 lexical `starts_with` 说成 symlink 防线。
- **`crates/ng-static/src/router.rs:300`**：ETag 为 `"{mtime_secs}-{size}"`——弱校验器。mtime 秒级 + 相同 size 的两个不同文件会碰撞；保持 size 与亚秒级 mtime 的内容变更不会让客户端缓存失效（返回 304）。对静态服务可接受，但**不是**内容哈希。
- **`crates/ng-static/src/ops.rs:621`**：`rename_file` 要求源存在（否则 NotFound），from/to 均须安全解析到同一 bucket 根下。**不支持跨 bucket 重命名**，即便 token 对两个 bucket 均有权限——设计如此（两条路径锚定同一 `model.path`）。
- **`crates/ng-static/src/rpc/static_bucket_file/rename_file.rs:36`**：rename RPC 顺序检查 Write 和 Delete（两次 `check_token_limit`）。授予 Write 但非 Delete 的 token 无法 rename，正确阻止绕过 Delete——但这是 RPC 层重复强制；底层 `rename_file` ops fn **不**复查权限，未来任何直接调用者将跳过鉴权。

## 依赖关系

ng-static 在 workspace 内依赖 `ng-core`（`FileInfo`、`NodegetError`、`Scope`/`Permission`/`StaticBucketPermission`/`StaticBucketFilePermission`、`PermissionChecker` trait、`TokenOrAuth`、`Limit`）、`ng-db`（`static_entity` + SeaORM entity）、`ng-infra`（`server` feature：`DbBackedCache`、`make_global_cache!`、`rpc_exec!`、`RpcHelper`、`token_identity`、`anyhow_to_nodeget_error`）、`ng-config`（server config 读取 static_path）。server 二进制依赖 `ng-static/server` 并通过 `rpc_nodeget.rs::build_modules()` 合并 `rpc_module()`、通过 `serve.rs` 注册 `router()`。agent **不**依赖 ng-static（其 `default=[]` 仅含 agent 安全的 `FileInfo` 类型）。

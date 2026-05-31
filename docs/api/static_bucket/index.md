# Static Bucket 静态文件服务（配置管理）

`static-bucket` 命名空间负责管理**静态文件服务的配置（Bucket）**，包括创建、读取、更新、删除 bucket，以及列出所有 bucket 名称。

::: info Bucket 与文件的分离
本命名空间**不操作磁盘文件**，只操作数据库中的配置记录。真正的文件上传、读取、删除、列出在 [
`static-bucket-file`](../static_bucket_file/index.md) 命名空间中完成。
:::

## 权限结构

- **Scope**：`StaticBucket(name)` 或 `Global`
    - `StaticBucket(name)`：只对该 bucket 生效
    - `Global`：对所有 bucket 生效（可用 `*` 通配）
- **Permission**：
    - `StaticBucket::Read` — 读取 bucket 配置
    - `StaticBucket::Write` — 创建 / 更新 bucket 配置
    - `StaticBucket::Delete` — 删除 bucket 配置

Config 中的 `static_path` 字段决定所有 bucket 的顶级磁盘根目录（默认 `./static/`）。

## 方法概览

| 方法名                                      | 功能             | 所需权限                   |
|------------------------------------------|----------------|------------------------|
| [static-bucket_create](./crud.md#create) | 创建 bucket 配置   | `StaticBucket::Write`  |
| [static-bucket_read](./crud.md#read)     | 读取 bucket 配置   | `StaticBucket::Read`   |
| [static-bucket_update](./crud.md#update) | 更新 bucket 配置   | `StaticBucket::Write`  |
| [static-bucket_delete](./crud.md#delete) | 删除 bucket 配置   | `StaticBucket::Delete` |
| [static-bucket_list](./crud.md#list)     | 列出所有 bucket 名称 | **仅 SuperToken**       |

## `Static` 数据结构

```json
{
  "id": 1,
  "name": "my-site",
  "path": "sites/my-site",
  "is_http_root": false,
  "cors": true,
  "enable": null
}
```

字段说明：

- `id`：数据库自增 ID
- `name`：RPC / URL 标识符，全局唯一（与磁盘路径无关）
- `path`：磁盘相对路径（相对 `static_path`），如 `sites/my-site`，允许 `/` 多级嵌套
- `is_http_root`：是否接管根路由 `/`
- `cors`：是否开启 `Access-Control-Allow-Origin: *`
- `enable`：是否启用 HTTP 访问。`false` 时该 bucket 的 HTTP 入口返回 404（不影响 RPC 操作和 WebDAV）

## 注意事项

- `name` 只作为 RPC 参数 / URL 标识符，不会拼接到磁盘路径
- `path` 字段才是决定磁盘位置的关键，实际磁盘根 = `{static_path(config)}/{path}/`
- `path` 允许 `/` 分隔多级子目录（如 `"sites/blog"`），每段必须符合 `[A-Za-z0-9_.-]`
- `is_http_root` 在同一时刻只能存在一个。尝试创建或更新第二条 `is_http_root` 为 `true` 的配置会返回错误（应用层查询逻辑保证 is_http_root 唯一性，无数据库层唯一约束）。
- 未绑定 `is_http_root` 时，根路由继续返回默认的占位 HTML
- **WebDAV 端点**：可通过 `/nodeget/static-webdav/{name}` 使用 WebDAV
  协议管理文件（详见 [Static Bucket File 文档](../static_bucket_file/index.md)），支持 mount 为网络盘

## 缓存说明

静态服务配置表会在程序启动时**全量加载到内存**。所有 `static-bucket_read` 和 HTTP 路由直接读取内存缓存，无需访问数据库。

`static-bucket_create`、`static-bucket_update`、`static-bucket_delete` 会在写库成功后自动重新加载内存缓存，确保一致性。

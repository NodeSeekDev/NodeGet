# Static Bucket CRUD 操作

## Create

创建一条静态文件服务 Bucket 配置，同时在磁盘上初始化对应目录。

### 方法

调用方法名为 `static-bucket_create`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "name": "my-site",
  "path": "sites/my-site",
  "is_http_root": false,
  "cors": true
}
```

### 权限要求

- Permission: `StaticBucket::Write`
- Scope: `StaticBucket(name)` 或 `Global`

### 返回值

创建成功时返回完整的静态配置对象：

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

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "static-bucket_create",
  "params": {
    "token": "demo_token",
    "name": "my-site",
    "path": "sites/my-site",
    "is_http_root": false,
    "cors": true
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "id": 1,
    "name": "my-site",
    "path": "sites/my-site",
    "is_http_root": false,
    "cors": true,
    "enable": null
  }
}
```

> 注：`enable` 字段创建时默认为 `null`（未启用），需通过 [`update`](#update) 显式设置为 `true`。

## Read

读取指定名称的静态服务 Bucket 配置。直接从内存缓存返回，性能极高。

### 方法

调用方法名为 `static-bucket_read`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "name": "my-site"
}
```

### 权限要求

- Permission: `StaticBucket::Read`
- Scope: `StaticBucket(name)` 或 `Global`

### 返回值

返回配置对象；若不存在返回 JSON-RPC 错误（code 105, NotFound）。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "static-bucket_read",
  "params": {
    "token": "demo_token",
    "name": "my-site"
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "id": 1,
    "name": "my-site",
    "path": "sites/my-site",
    "is_http_root": false,
    "cors": true,
    "enable": true
  }
}
```

## Update

更新现有静态服务 Bucket 配置。

### 方法

调用方法名为 `static-bucket_update`，参数与 `static-bucket_create` 相同。

### 权限要求

- Permission: `StaticBucket::Write`
- Scope: `StaticBucket(name)` 或 `Global`

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "static-bucket_update",
  "params": {
    "token": "demo_token",
    "name": "my-site",
    "path": "sites/my-site",
    "is_http_root": true,
    "cors": true,
    "enable": false
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "id": 1,
    "name": "my-site",
    "path": "sites/my-site",
    "is_http_root": true,
    "cors": true,
    "enable": false
  }
}
```

## Delete

删除指定名称的静态服务 Bucket 配置。**不会删除磁盘上的文件**。

### 方法

调用方法名为 `static-bucket_delete`，需要提供以下参数：

```json
{
  "token": "demo_token",
  "name": "my-site"
}
```

### 权限要求

- Permission: `StaticBucket::Delete`
- Scope: `StaticBucket(name)` 或 `Global`

### 返回值

删除成功返回 `{"success": true}`。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "static-bucket_delete",
  "params": {
    "token": "demo_token",
    "name": "my-site"
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "success": true
  }
}
```

## List

列出所有已创建的静态服务 Bucket 名称。

### 方法

调用方法名为 `static-bucket_list`，需要提供以下参数：

```json
{
  "token": "demo_super_token" // Super Token
}
```

### 权限要求

只有 **Super Token** 可以调用该方法。普通 Token 会返回权限错误。

数据来源为内存缓存，不会访问数据库或磁盘。

### 返回值

返回所有静态服务 `name` 字段组成的数组，**按字典序排序**：

```json
["api-docs", "blog", "my-site"]
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "static-bucket_list",
  "params": {
    "token": "demo_super_token"
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": ["api-docs", "blog", "my-site"]
}
```

# Db 命名空间

提供本地数据库实例的 CRUD 管理 + SQL 执行能力

## 方法列表

| 方法名 | 描述 | 权限要求 |
|--------|------|----------|
| [create](#create) | 创建新的本地数据库 | `Db::Create` |
| [read](#read) | 获取数据库元信息 | `Db::Read` |
| [update](#update) | 重命名数据库 | `Db::Update` |
| [delete](#delete) | 删除数据库 | `Db::Delete` |
| [list](#list) | 列出所有数据库 | `Db::List` |
| [exec_sql](#exec-sql) | 执行原始 SQL（含参数） | `Db::ExecSql` |
| [exec_templating](#exec-templating) | 参数化 SQL 执行，参数必须为数组 | `Db::ExecSql` |

## 数据库存储

默认在 `./db/` 目录下，每个数据库为独立的 SQLite 文件 `{name}.db`。

通过 `ServerConfig.db_path` 可自定义根目录。

## 权限体系

数据库操作使用独立的 `Db` 权限枚举，与主 `NodeGet` 权限体系并列：

```rust
pub enum Db {
    List,     // 列出所有数据库
    Read,     // 读取数据库元信息
    Create,   // 创建新数据库
    Update,   // 重命名数据库
    Delete,   // 删除数据库
    ExecSql,  // 对数据库执行 SQL
}
```

## Create

创建一个新的本地数据库实例。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_create",
  "params": ["TOKEN", "my_database"],
  "id": 1
}
```

**响应 (成功):**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "success": true,
    "data": {
      "name": "my_database",
      "file_path": "./db/my_database.db"
    }
  },
  "id": 1
}
```

## Read

获取数据库实例元信息。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_read",
  "params": ["TOKEN", "my_database"],
  "id": 1
}
```

**响应 (成功):**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "success": true,
    "data": {
      "id": 1,
      "name": "my_database",
      "created_at": 1748102400000,
      "active": true
    }
  },
  "id": 1
}
```

## Update

重命名数据库实例。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_update",
  "params": ["TOKEN", "old_name", "new_name"],
  "id": 1
}
```

**响应 (成功):**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "success": true,
    "data": {
      "id": 1,
      "name": "new_name",
      "created_at": 1748102400000
    }
  },
  "id": 1
}
```

## Delete

删除数据库实例并清理磁盘文件。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_delete",
  "params": ["TOKEN", "my_database"],
  "id": 1
}
```

**响应 (成功):**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "success": true
  },
  "id": 1
}
```

## List

列出所有已注册的数据库实例。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_list",
  "params": ["TOKEN"],
  "id": 1
}
```

**响应:**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "success": true,
    "data": [
      {
        "id": 1,
        "name": "my_database",
        "file_path": "./db/my_database.db",
        "db_connections": 1,
        "max_lifetime_ms": null,
        "created_at": 1748102400000,
        "is_active": true
      }
    ]
  },
  "id": 1
}
```

**字段说明:**

| 字段               | 类型        | 说明                                 |
|--------------------|-------------|--------------------------------------|
| `id`               | i64         | db_registry 表中的主键 ID            |
| `name`             | String      | 数据库名称                           |
| `file_path`        | String      | SQLite 文件在磁盘上的路径            |
| `db_connections`   | Option<i32> | 当前活跃连接数                       |
| `max_lifetime_ms`  | Option<i64> | 连接空闲超时时间（毫秒），null=永不超时 |
| `created_at`       | i64         | 创建时间戳（毫秒）                   |
| `is_active`        | bool        | 是否正在连接池中                     |

## Exec Sql

对指定本地数据库执行原始 SQL。支持 `$1, $2` 参数化查询。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_exec_sql",
  "params": ["TOKEN", "my_database", "SELECT * FROM users WHERE age > $1", [18]],
  "id": 1
}
```

**响应 (SELECT):**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "success": true,
    "data": [
      {"id": 1, "name": "Alice", "age": 25}
    ],
    "row_count": 1
  },
  "id": 1
}
```

**响应 (DML/DDL):**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "success": true,
    "data": [],
    "row_count": 3
  },
  "id": 1
}
```

## Exec Templating

参数化 SQL。与 `exec_sql` 功能相同，但要求 `params` 必须为数组。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_exec_templating",
  "params": ["TOKEN", "my_database", "SELECT * FROM users WHERE name = $1", ["Alice"]],
  "id": 1
}
```

响应格式同 `exec_sql`。

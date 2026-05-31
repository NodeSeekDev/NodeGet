# 本地数据库管理 (db 命名空间)

管理多个独立 SQLite 数据库实例，支持对每个数据库执行原始 SQL 和参数化查询。

所有方法均位于 `db` 命名空间下。

## 方法列表

| 方法名                                 | 描述                  | 权限要求          |
|-------------------------------------|---------------------|---------------|
| [create](#create)                   | 创建新的本地数据库           | `Db::Create`  |
| [read](#read)                       | 获取数据库元信息            | `Db::Read`    |
| [update](#update)                   | 重命名数据库              | `Db::Update`  |
| [delete](#delete)                   | 删除数据库并清理文件          | `Db::Delete`  |
| [list](#list)                       | 列出所有数据库             | `Db::List`    |
| [exec_sql](#exec-sql)               | 执行原始 SQL（允许参数和复合语句） | `Db::ExecSql` |
| [exec_templating](#exec-templating) | 参数化 SQL 执行，参数必须为数组  | `Db::ExecSql` |

## 数据库存储

所有本地数据库文件默认存放在配置文件 `db_path` 指定的目录下，格式为 `{db_path}/{name}.db`（SQLite）。

配置文件默认路径为 `./db/`。

## 连接池管理

服务端维护一个 `DbRegistryManager` 连接池，对每个注册的数据库实例缓存 `DatabaseConnection`。

当某个数据库超过 `max_lifetime_ms` 毫秒未被访问时，连接池会自动关闭并回收该连接。

## 权限

本地数据库使用独立的 `Db` 权限体系，与 `NodeGet` 主权限体系并列。Token 需要拥有对应的 `Db::Xxx` 权限才能执行相应操作。

```rust
pub enum Db {
    List,
    Read,
    Create,
    Update,
    Delete,
    ExecSql,
}
```

授权 Scope 为 `Scope::Db(name)`（即权限仅对指定数据库实例生效）。只有 `db_list` 方法使用 `Scope::Global` + `Db::List` 权限。

---

## Create

创建一个新的本地数据库实例。会在 `db_path` 目录下创建 `{name}.db` SQLite 文件，同时在主数据库的 `db_registry` 表中注册一条记录。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_create",
  "params": {
    "token": "ADMIN:SECRET",
    "name": "my_database"
  },
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

**错误码:**

| 代码  | 说明                         |
|-----|----------------------------|
| 102 | Permission Denied          |
| 108 | Invalid input (name 含非法字符) |
| 103 | Database error (已存在)       |

---

## Read

获取数据库实例的元信息（注册状态、文件路径、是否活跃在连接池中等）。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_read",
  "params": {
    "token": "TOKEN",
    "name": "my_database"
  },
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

**错误码:**

| 代码  | 说明                 |
|-----|--------------------|
| 102 | Permission Denied  |
| 105 | Database not found |

---

## Update

重命名数据库实例。会同时重命名磁盘上的 `.db` 文件和连接池中的条目。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_update",
  "params": {
    "token": "TOKEN",
    "name": "old_name",
    "new_name": "new_name"
  },
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

**错误码:**

| 代码  | 说明                  |
|-----|---------------------|
| 102 | Permission Denied   |
| 105 | Database not found  |
| 108 | new_name 已存在        |
| 101 | I/O error (重命名文件失败) |

---

## Delete

删除数据库实例。从连接池中移除、从 `db_registry` 表中删除记录、并删除磁盘上的 `.db` / `.db-wal` / `.db-shm` 文件。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_delete",
  "params": {
    "token": "TOKEN",
    "name": "my_database"
  },
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

**错误码:**

| 代码  | 说明                 |
|-----|--------------------|
| 102 | Permission Denied  |
| 105 | Database not found |

---

## List

列出所有已注册的数据库实例。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_list",
  "params": {
    "token": "TOKEN"
  },
  "id": 1
}
```

**响应 (成功):**

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
      },
      {
        "id": 2,
        "name": "another_db",
        "file_path": "./db/another_db.db",
        "db_connections": 0,
        "max_lifetime_ms": 3600000,
        "created_at": 1748102500000,
        "is_active": false
      }
    ]
  },
  "id": 1
}
```

**字段说明:**

| 字段                | 类型          | 说明                     |
|-------------------|-------------|------------------------|
| `id`              | i64         | db_registry 表中的主键 ID   |
| `name`            | String      | 数据库名称                  |
| `file_path`       | String      | SQLite 文件在磁盘上的路径（相对于工作目录） |
| `db_connections`  | `Option<i32>` | 当前活跃连接数                |
| `max_lifetime_ms` | `Option<i64>` | 连接空闲超时时间（毫秒），null=永不超时 |
| `created_at`      | i64         | 创建时间戳（毫秒）              |
| `is_active`       | bool        | 是否正在连接池中：创建后为 true，`get_conn()` 连接成功后为 true，超过 `max_lifetime_ms` 未被访问变为 false，为 false 时下次调用会自动重建启动连接池 |

---

## Exec Sql

对指定本地数据库执行原始 SQL 语句。支持参数化查询（SQL 中使用 `$1`, `$2` 占位符，SQLite 和 PostgreSQL 都支持该格式）。

SELECT / PRAGMA / EXPLAIN / WITH 语句自动返回结果行，其余语句（INSERT / UPDATE / DELETE / DDL / PRAGMA 写操作等）返回影响行数。

### 注意事项

- SQL 类型判断使用 `trim_start_matches` + `starts_with_ascii_ci` 后检查前缀
- 多语句（含 `;`）**不被禁止**，由数据库引擎自行处理
- 参数化查询防止 SQL 注入
- 使用 `$1`, `$2`... 格式的命名参数（SQLite 和 PostgreSQL 均支持）

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_exec_sql",
  "params": {
    "token": "TOKEN",
    "name": "my_database",
    "sql": "SELECT * FROM users WHERE age > $1",
    "params": [18]
  },
  "id": 1
}
```

**响应 (成功):**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "success": true,
    "data": [
      {
        "id": 1,
        "name": "Alice",
        "age": 25,
        "email": "alice@example.com"
      },
      {
        "id": 2,
        "name": "Bob",
        "age": 30,
        "email": "bob@example.com"
      }
    ],
    "row_count": 2
  },
  "id": 1
}
```

对于 DML 操作 (INSERT/UPDATE/DELETE/DDL)，`data` 为空数组，`row_count` 为受影响的行数:

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

**参数说明:**

| 参数    | 类型              | 说明                            |
|-------|-----------------|-------------------------------|
| token | String          | Token 字符串                     |
| name  | String          | 目标数据库名称                       |
| sql   | String          | SQL 语句（支持 `$1, $2` 占位符）       |
| params | `array` / `null` | 参数数组，对应 `$1, $2`...。可传 `null` |

**JSON 参数类型映射到 SeaORM Value:**

| JSON 类型        | SeaORM Value         | 说明            |
|----------------|----------------------|---------------|
| `null`         | `Value::Json(None)`  |               |
| `bool`         | `Value::Bool`        |               |
| `number` (i64) | `Value::BigInt`      |               |
| `number` (u64) | `Value::BigUnsigned` |               |
| `number` (f64) | `Value::Double`      |               |
| `string`       | `Value::String`      |               |
| `array/object` | `Value::Json`        | 序列化为 JSONB 存储 |

**错误码:**

| 代码  | 说明                                 |
|-----|------------------------------------|
| 102 | Permission Denied                  |
| 103 | Database error (数据库未注册 / SQL 执行失败) |
| 108 | Invalid input (参数格式错误)             |

---

## Exec Templating

参数化 SQL 执行。与 `exec_sql` 功能相同，支持 `params` 为数组或 `null`（null 视为空数组）。

这是推荐的接口，用于确保所有用户输入都通过参数化查询传入，防止 SQL 注入。

**请求:**

```json
{
  "jsonrpc": "2.0",
  "method": "db_exec_templating",
  "params": {
    "token": "TOKEN",
    "name": "my_database",
    "sql": "SELECT * FROM users WHERE name = $1",
    "params": ["Alice"]
  },
  "id": 1
}
```

**响应格式** 与 `exec_sql` 完全一致。

**错误码:**

| 代码  | 说明                      |
|-----|-------------------------|
| 108 | params 格式错误（必须为数组或null） |
| 其他  | 与 exec_sql 相同           |

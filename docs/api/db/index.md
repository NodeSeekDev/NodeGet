# Db 命名空间

提供本地数据库实例的 CRUD 管理 + SQL 执行能力。

## 概述

Db 命名空间允许在服务端创建和管理多个独立的 SQLite 数据库实例，并对每个实例执行原始 SQL 和参数化查询。

所有本地数据库文件默认存放在配置文件 `db_path` 指定的目录下，格式为 `{db_path}/{name}.db`。

## 方法列表

| 方法名                                 | 描述                  | 权限要求          |
|-------------------------------------|---------------------|---------------|
| [create](./crud.md#create)          | 创建新的本地数据库           | `Db::Create`  |
| [read](./crud.md#read)              | 获取数据库元信息            | `Db::Read`    |
| [update](./crud.md#update)          | 重命名数据库              | `Db::Update`  |
| [delete](./crud.md#delete)          | 删除数据库并清理文件          | `Db::Delete`  |
| [list](./crud.md#list)              | 列出所有数据库             | `Db::List`    |
| [exec_sql](./crud.md#exec-sql)      | 执行原始 SQL（允许参数和复合语句） | `Db::ExecSql` |
| [exec_templating](./crud.md#exec-templating) | 参数化 SQL 执行，参数必须为数组  | `Db::ExecSql` |

## 数据库存储

所有本地数据库文件默认存放在配置文件 `db_path` 指定的目录下，格式为 `{db_path}/{name}.db`（SQLite）。

配置文件默认路径为 `./db/`。

## 权限体系

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

各方法的详细请求/响应格式与示例，请参阅 [Db CRUD 操作](./crud.md)。

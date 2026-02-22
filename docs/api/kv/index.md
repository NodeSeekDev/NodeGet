# Kv 键值数据库总览

我们在 Nodeget Server 实现了一个简单的 Key-Value 数据储存

可用于前端配置存储，节点 Metadata 信息存储等

## 基本结构体

在数据库中每一行的基本结构体如下:

```rust
// 每个 KVStore 代表一个命名空间，包含一个 HashMap 存储键值对
/// 其中 key 是字符串，value 是任意 JSON 值
pub struct KVStore {
    // 命名空间名称，作为唯一标识符
    namespace: String,
    // 存储键值对的 HashMap
    kv: HashMap<String, serde_json::Value>,
}
```

Value 可以是合法的任意 Json 值，在数据库内会以 JsonBinary 的形式储存，所以请不要依赖其顺序性与重复性

## 基本权限

Kv 的权限结构与普通的 Token 权限略有不同:

```rust
pub enum Scope {
    Global,
    AgentUuid(uuid::Uuid),

    // KvNamespace 作用域，通过名称指定
    // 不建议与写在同一个 Limit 里面，一个 Token 可对应多个 Limit
    KvNamespace(String),
}
```

```rust
pub enum Permission {
    // 其他

    // Kv 权限
    Kv(Kv),
}
```

```rust
pub enum Kv {
    ListAllKeys,
    Read(String),
    Write(String),
    Delete(String),
}
```

### 注意事项

`ListAllKeys` 可以列出在这一 KvNamespace Scope 下的所有键 (但是不一定可以读取键对应的值)

`Read` / `Write` / `Delete` 的 String，可以拥有通配符，比如 `metadata_*`，表达可以操作 这一 KvNamespace Scope 下的所有以 `metadata_` 开头的键

## 权限 Demo Json

```json
{
    "scopes": [
        {
            "kv_namespace": "kv_test"
        }
    ],
    "permissions": [
        {
            "kv": "list_all_keys"
        },
        {
            "kv": {
                "read": "metadata_*"
            }
        },
        {
            "kv": {
                "write": "metadata_*"
            }
        },
        {
            "kv": {
                "delete": "metadata_*"
            }
        }
    ]
}
```

该权限表示，在 `kv_test` Namespace 的 Kv 下，可以列出所有的 Keys，并 读写删除 以 `metadata_` 开头的键

## 特殊 Kv 与 特殊键

### 特殊 Kv

每一个 Agent 都建议在 Kv 系统中拥有以自身 Uuid 为 Name 的 Kv Namespace，但该 Kv 并非必须，也不会自动创建

### 特殊键

在一个 Kv 中，非 Agent / Server 开发者不建议使用以下的键，其在 Agent / Server 内部有特殊用途，或为共同认定的功能键

- `database_limit_*`:
  - `database_limit_static_monitoring`: 单位毫秒
        在以 Agent Uuid 为命名的 Kv 中，设置该值则表示:

        Crontab 执行 Server CleanUpDatabse 任务时，在 Static 表中查询最后一个该 Uuid 的数据，获取其 Timestamp
        
        清理 `从 (Timestamp - 该值) 至 Timestamp` **以外的**数据，可以理解为清理旧数据，保留新数据
        
        该设置不受数据条数影响，仅以 Timestamp 为标准

        若某一 Agent 设置了该值，并在历史某一时刻不再上传数据，则不会影响其 `从 (最后一个 Timestamp - 该值) 至 最后一个 Timestamp` 的数据
  - `database_limit_dynamic_monitoring`: 同上，Dynamic Monitoring Data
  - `database_limit_task`: 同上，Task 记录
- `metadata_*`
  - `metadata_name`: 前端展示 Agent 名字
  - `metadata_tags`: 前端展示的 Tag，为数组，值为 String，如: `["tag1", "tag2"]`
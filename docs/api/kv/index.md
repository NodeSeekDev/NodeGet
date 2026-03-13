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
    ListAllNamespace,
    ListAllKeys,
    Read(String),
    Write(String),
    Delete(String),
}
```

### 注意事项

`ListAllNamespace` 可以列出当前 Token 有权限看到的 Kv Namespace

在 `Global` Scope 下拥有该权限时，可列出所有 Namespace；在 `KvNamespace(xxx)` Scope 下拥有该权限时，仅可列出对应的 `xxx`

`ListAllKeys` 可以列出在这一 KvNamespace Scope 下的所有键 (但是不一定可以读取键对应的值)

`Read` / `Write` / `Delete` 的 String，可以拥有通配符，比如 `metadata_*`，表达可以操作 这一 KvNamespace Scope 下的所有以
`metadata_` 开头的键

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


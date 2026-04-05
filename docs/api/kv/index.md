# Kv 键值数据库总览

我们在 Nodeget Server 实现了一个简单的 Key-Value 数据储存

可用于前端配置存储，节点 Metadata 信息存储等

## 方法列表

| 方法名 | 描述 |
|-------|------|
| [kv_set_value](./crud.md#set-key-value) | 写入键值对 |
| [kv_get_value](./crud.md#get-key-value) | 读取单个键值 |
| [kv_get_multi_value](./crud.md#get-multi-key-value) | 批量读取键值 |
| [kv_delete_key](./crud.md#delete-key-value) | 删除键值对 |
| [kv_get_all_keys](./crud.md#list-all-keys) | 列出所有键名 |
| [kv_create_namespace](./crud.md#create-namespace) | 创建命名空间 |
| [kv_list_all_namespace](./crud.md#list-all-namespace) | 列出所有命名空间 |

特殊说明请参考 [special.md](./special.md)。

## 基本结构体

在数据库中每一行的基本结构体如下:

```rust
pub struct KVStore {
    namespace: String,                    // 命名空间名称，作为唯一标识符
    kv: HashMap<String, serde_json::Value>, // 存储键值对的 HashMap
}
```

Value 可以是合法的任意 Json 值，在数据库内会以 JsonBinary 的形式储存，所以请不要依赖其顺序性与重复性

## 基本权限

Kv 的权限结构与普通的 Token 权限略有不同:

```rust
pub enum Scope {
    Global,
    AgentUuid(uuid::Uuid),
    KvNamespace(String), // KvNamespace 作用域，通过名称指定
}
```

```rust
pub enum Permission {
    // 其他权限...
    Kv(Kv),
}
```

```rust
pub enum Kv {
    ListAllNamespace,
    ListAllKeys,
    Read(String),   // 支持通配符 *
    Write(String),  // 支持通配符 *
    Delete(String), // 支持通配符 *
}
```

### 注意事项

`ListAllNamespace` 可以列出当前 Token 有权限看到的 Kv Namespace

在 `Global` Scope 下拥有该权限时，可列出所有 Namespace；在 `KvNamespace(xxx)` Scope 下拥有该权限时，仅可列出对应的 `xxx`

`ListAllKeys` 可以列出在这一 KvNamespace Scope 下的所有键 (但是不一定可以读取键对应的值)

`Read` / `Write` / `Delete` 的 String，可以拥有通配符，比如 `metadata_*`，表达可以操作这一 KvNamespace Scope 下的所有以 `metadata_` 开头的键

`kv_get_multi_value` 支持批量读取，并支持在请求 key 中直接使用后缀通配符（如 `metadata_*`）

## 权限 Demo Json

```json
{
  "scopes": [
    {
      "kv_namespace": "kv_test" // 指定 KvNamespace 作用域
    }
  ],
  "permissions": [
    {
      "kv": "list_all_keys" // 列出所有键
    },
    {
      "kv": {
        "read": "metadata_*" // 读取 metadata_ 开头的键
      }
    },
    {
      "kv": {
        "write": "metadata_*" // 写入 metadata_ 开头的键
      }
    },
    {
      "kv": {
        "delete": "metadata_*" // 删除 metadata_ 开头的键
      }
    }
  ]
}
```

该权限表示，在 `kv_test` Namespace 的 Kv 下，可以列出所有的 Keys，并读写删除以 `metadata_` 开头的键

# Key-Value CRUD

## Set Key-Value

写入一个键值对到指定的 Namespace 中。

### 方法

调用方法名为 `kv_set_value`，需要提供以下参数：

```json
{
  "token": "demo_token",   // Token
  "namespace": "kv_test",  // 命名空间
  "key": "metadata_test",  // 键名
  "value": [               // 任意类型 Json 数据
    12312313213
  ]
}
```

### 权限要求

- Permission: `Kv::Write(key)` 需要覆盖目标 key（支持通配符匹配）
- Scope: `KvNamespace(namespace)` 或 `Global`

### 返回值

写入成功时返回 `{"success": true}`。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "kv_set_value",
  "params": {
    "token": "demo_token",   // Token
    "namespace": "kv_test",  // 命名空间
    "key": "metadata_test",  // 键名
    "value": [               // 任意类型 Json 数据
      12312313213
    ]
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

## Get Key-Value

读取指定 Namespace 下单个 Key 对应的值。

### 方法

调用方法名为 `kv_get_value`，需要提供以下参数：

```json
{
  "token": "demo_token",   // Token
  "namespace": "kv_test",  // 命名空间
  "key": "metadata_test"   // 键名
}
```

### 权限要求

- Permission: `Kv::Read(key)` 需要覆盖目标 key（支持通配符匹配）
- Scope: `KvNamespace(namespace)` 或 `Global`

### 返回值

返回该 Key 对应的 JSON 值：

```json
[
  12312313213
]
```

若 Key 不存在，返回 `null`。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "kv_get_value",
  "params": {
    "token": "demo_token",   // Token
    "namespace": "kv_test",  // 命名空间
    "key": "metadata_test"   // 键名
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    12312313213
  ]
}
```

## Get Multi Key-Value

按请求数组批量读取多个 Namespace 下的多个 Key。

### 方法

调用方法名为 `kv_get_multi_value`，需要提供以下参数：

```json
{
  "token": "demo_token", // Token
  "namespace_key": [
    {
      "namespace": "ns1", // 命名空间
      "key": "key1"       // 精确 key
    },
    {
      "namespace": "ns1",
      "key": "metadata_*" // 通配符 key，匹配所有 metadata_ 开头的键
    },
    {
      "namespace": "ns2",
      "key": "key2"
    }
  ]
}
```

参数规则：

- `namespace_key` 为数组，可传任意数量项
- 每一项格式固定为 `{namespace, key}`
- `key` 支持后缀通配符 `*`，例如 `metadata_*`
- 仅支持后缀通配符（`*` 只能出现一次且必须在末尾）

### 权限要求

- 每一项都必须在其 `KvNamespace(namespace)` Scope 下具备对应 `Kv::Read(key)` 权限覆盖
- 只要有一项无权限，整体直接返回权限错误，不返回部分结果

### 返回值

返回数组结构：

```json
[
  {
    "namespace": "ns1",
    "key": "key1",
    "value": "value1"       // 精确 key 的值
  },
  {
    "namespace": "ns1",
    "key": "metadata_cpu",
    "value": 10              // 通配符匹配的键值
  },
  {
    "namespace": "ns1",
    "key": "metadata_ram",
    "value": 20              // 通配符匹配的键值
  }
]
```

说明：

- 返回顺序按请求顺序拼接
- 对于通配符项，命中的 key 按字典序返回
- 对于精确 key，若不存在会返回 `value: null`

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "kv_get_multi_value",
  "params": {
    "token": "demo_token", // Token
    "namespace_key": [
      {
        "namespace": "ns1", // 命名空间
        "key": "key1"       // 精确 key
      },
      {
        "namespace": "ns1",
        "key": "metadata_*" // 通配符 key
      }
    ]
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "namespace": "ns1",
      "key": "key1",
      "value": "value1"
    },
    {
      "namespace": "ns1",
      "key": "metadata_cpu",
      "value": 10
    },
    {
      "namespace": "ns1",
      "key": "metadata_ram",
      "value": 20
    }
  ]
}
```

## Delete Key-Value

删除指定 Namespace 下的一个键值对。

### 方法

调用方法名为 `kv_delete_key`，需要提供以下参数：

```json
{
  "token": "demo_token",   // Token
  "namespace": "kv_test",  // 命名空间
  "key": "metadata_test"   // 要删除的键名
}
```

### 权限要求

- Permission: `Kv::Delete(key)` 需要覆盖目标 key（支持通配符匹配）
- Scope: `KvNamespace(namespace)` 或 `Global`

### 返回值

删除成功时返回 `{"success": true}`。

若 Key 不存在，同样返回 `{"success": true}`（不报错）。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "kv_delete_key",
  "params": {
    "token": "demo_token",   // Token
    "namespace": "kv_test",  // 命名空间
    "key": "metadata_test"   // 要删除的键名
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

## Delete Namespace

删除整个 Namespace，包括该 Namespace 下的所有键值对。

### 方法

调用方法名为 `kv_delete_namespace`，需要提供以下参数：

```json
{
  "token": "demo_token",   // Token
  "namespace": "kv_test"   // 要删除的命名空间
}
```

### 权限要求

- Permission: `Kv::Delete("*")` 需要对该 Namespace 拥有全局删除权限
- Scope: `KvNamespace(namespace)` 或 `Global`

### 返回值

删除成功时返回 `{"success": true}`。

若 Namespace 不存在，返回错误。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "kv_delete_namespace",
  "params": {
    "token": "demo_token",   // Token
    "namespace": "kv_test"   // 要删除的命名空间
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

## List All Keys

列出指定 Namespace 下的所有键名。

### 方法

调用方法名为 `kv_get_all_keys`，需要提供以下参数：

```json
{
  "token": "demo_token",  // Token
  "namespace": "kv_test"  // 命名空间
}
```

### 权限要求

- Permission: `Kv::ListAllKeys`
- Scope: `KvNamespace(namespace)` 或 `Global`

### 返回值

返回一个字符串数组，每个元素是一个 Key 名称（包含内部占位符 `__nodeget_namespace_marker__`）：

```json
[
  "__nodeget_namespace_marker__",
  "config_theme",
  "metadata_name",
  "metadata_test"
]
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "kv_get_all_keys",
  "params": {
    "token": "demo_token",  // Token
    "namespace": "kv_test"  // 命名空间
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    "__nodeget_namespace_marker__",
    "config_theme",
    "metadata_name",
    "metadata_test"
  ]
}
```

## Create Namespace

创建一个新的 Kv Namespace，该操作仅限 Super Token 使用。

创建 Namespace 的本质是在 KV 表中写入一条 key 为 `__nodeget_namespace_marker__`、value 为 `null` 的占位记录。Server 通过该记录判断 Namespace 是否存在。

该 key 是公开可见的：通过 `kv_get_all_keys` 和 `kv_get_value` 均可读取到。用户也可以通过 `kv_set_value` 修改其 value，或通过 `kv_delete_key` 删除它（删除后该 Namespace 将被视为不存在）。

### 方法

调用方法名为 `kv_create`，需要提供以下参数：

```json
{
  "token": "demo_token", // Super Token
  "namespace": "kv_test" // 要创建的命名空间名称
}
```

### 权限要求

- 该方法仅限 Super Token 调用
- 普通 Token 无论拥有何种权限，均无法创建 Namespace

### 返回值

创建成功时返回创建的命名空间信息：

```json
{
  "id": 1,
  "namespace": "kv_test",
  "kv": {}
}
```

若命名空间已存在，会返回错误。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "kv_create",
  "params": {
    "token": "demo_token", // Super Token
    "namespace": "kv_test" // 要创建的命名空间名称
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
    "namespace": "kv_test",
    "kv": {}
  }
}
```

## List All Namespace

列出当前 Server 中可访问的 Kv Namespace。

### 方法

调用方法名为 `kv_list_all_namespace`，需要提供以下参数：

```json
{
  "token": "demo_token" // Token
}
```

### 权限要求

- Permission: `Kv::ListAllNamespace`
- Scope 规则:
    - 在 `Global` Scope 下拥有该权限: 可以列出所有 Namespace
    - 在 `KvNamespace(xxx)` Scope 下拥有该权限: 仅能列出该 Scope 对应的 Namespace
    - 未拥有该权限: 返回权限错误

### 返回值

返回一个字符串数组，每个元素是一个 Namespace 名称：

```json
[
  "global",
  "frontend_nodeget",
  "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3"
]
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "kv_list_all_namespace",
  "params": {
    "token": "demo_token" // Token
  },
  "id": 1
}
```

响应:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    "global",
    "frontend_nodeget",
    "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3"
  ]
}
```

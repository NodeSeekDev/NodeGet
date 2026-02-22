# 错误处理 (Error Handling)

NodeGet 项目的 API 使用统一的错误处理机制，确保所有 RPC 调用都能提供一致的错误响应格式。

## 错误响应格式

所有 API 错误都遵循以下 JSON 结构：

```json
{
  "error": {
    "code": <error_code>,
    "message": "<error_message>",
    "data": <optional_error_data>
  }
}
```

## NodegetError 枚举

NodeGet 定义了一个统一的错误枚举 `NodegetError`，包含以下错误类型：

### 错误类型

- **ParseError(String)** - 解析错误 (错误码: 101)
    - 当请求数据解析失败时抛出

- **PermissionDenied(String)** - 权限拒绝 (错误码: 102)
    - 当用户没有执行操作的足够权限时抛出

- **DatabaseError(String)** - 数据库错误 (错误码: 103)
    - 当数据库操作失败时抛出

- **AgentConnectionError(String)** - 代理连接错误 (错误码: 104)
    - 当无法连接到代理节点时抛出

- **NotFound(String)** - 未找到 (错误码: 105)
    - 当请求的资源不存在时抛出

- **UuidNotFound(String)** - UUID 未找到 (错误码: 106)
    - 当请求的 UUID 不存在时抛出

- **ConfigNotFound(String)** - 配置未找到 (错误码: 107)
    - 当请求的配置不存在时抛出

- **SerializationError(String)** - 序列化错误 (错误码: 101)
    - 当数据序列化/反序列化失败时抛出

- **IoError(String)** - IO 错误 (错误码: 101)
    - 当输入/输出操作失败时抛出

- **Other(String)** - 其他错误 (错误码: 999)
    - 用于其他未分类的错误

## JsonError 结构体

为了统一错误响应格式，NodeGet 使用 `JsonError` 结构体：

```rust
pub struct JsonError {
    pub error_id: i128,           // 错误代码
    pub error_message: String,    // 错误消息
}
```

### 错误代码映射

- `101` - ParseError / SerializationError / IoError
- `102` - PermissionDenied
- `103` - DatabaseError
- `104` - AgentConnectionError
- `105` - NotFound
- `106` - UuidNotFound
- `107` - ConfigNotFound
- `999` - Other

## 错误处理示例

### 成功响应

```json
{
  "result": {
    // 成功的数据
  }
}
```

### 错误响应

```json
{
  "error": {
    "code": 102,
    "message": "Permission denied: Insufficient permissions to read requested task types"
  }
}
```

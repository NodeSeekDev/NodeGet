# Token CRUD

## Get Token

提供一个 Token，即可获取 Token 对应的详细信息结构体。

### 方法

调用方法名为 `token_get`，需要提供以下参数：

```json
{
  "token": "demo_token",            // 需要查询的 Token
  "supertoken": "SUPER_TOKEN"       // 可选，SuperToken，启用后允许 token 传入 username / token_key
}
```

- `token`: 需要查询的 Token，支持 `token_key:token_secret` 或 `username|password` 格式
- `supertoken`（可选）: SuperToken，启用后允许 `token` 传入 `username` / `token_key` 简写查询

当你持有 SuperToken 时，可以用简写查询指定 Token：

```json
{
  "token": "target_username_or_token_key", // 仅传入 username 或 token_key 即可
  "supertoken": "SUPER_TOKEN_KEY:SUPER_TOKEN_SECRET"
}
```

若 `supertoken` 存在，`token` 仍然支持完整格式（`token_key:token_secret` 或 `username|password`）。

### 权限要求

任何有效的 Token 均可查询自身信息。

若需要通过 `username` / `token_key` 简写查询其他 Token，需要提供 `supertoken` 参数。

### 返回值

返回值即为 Token 总览中的 Token 结构体:

```json
{
  "version": 1,                    // Token 版本
  "token_key": "n0kB8lSAykFd9Egu", // Token Key
  "timestamp_from": null,          // 有效期起始，毫秒时间戳
  "timestamp_to": null,            // 有效期结束，毫秒时间戳
  "token_limit": [                 // 权限列表
    {
      "scopes": [
        "global"                   // 全局作用域
      ],
      "permissions": [
        {
          "task": "listen"         // 监听任务
        },
        {
          "task": {
            "write": "ping"        // 上报 ping 任务
          }
        }
      ]
    }
  ],
  "username": null                 // 用户名
}
```

当 Token 具有 Crontab 权限时，返回值中可能会包含类似以下的权限信息：

```json
{
  "permissions": [
    {
      "crontab": "read"   // 读取 Crontab
    },
    {
      "crontab": "write"  // 创建 Crontab
    },
    {
      "crontab": "delete" // 删除 Crontab
    }
  ]
}
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "token_get",
  "params": {
    "token": "n0kB8lSAykFd9Egu:a0a7V3g43xjUCYIU5Md76H5QMPSlPPT6"
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
    "version": 1,
    "token_key": "n0kB8lSAykFd9Egu",
    "timestamp_from": null,
    "timestamp_to": null,
    "token_limit": [
      {
        "scopes": [
          "global"
        ],
        "permissions": [
          {
            "task": "listen"
          },
          {
            "task": {
              "write": "ping"
            }
          }
        ]
      }
    ],
    "username": null
  }
}
```

使用 SuperToken 简写查询:

```json
{
  "jsonrpc": "2.0",
  "method": "token_get",
  "params": {
    "token": "n0kB8lSAykFd9Egu",
    "supertoken": "ROOT_KEY:ROOT_SECRET"
  },
  "id": 2
}
```

## Create Token

只有 SuperToken 有权限创建 Token。

### 方法

调用方法名为 `token_create`，需要提供以下参数：

```json
{
  "father_token": "demo_super_token", // 父 Token，必须为 SuperToken
  "token_creation": {                 // TokenCreationRequest 结构体
    "username": "GM",                 // 可选，用户名
    "password": "ILoveRust1",         // 可选，密码
    "timestamp_from": null,           // 可选，有效期起始，毫秒时间戳
    "timestamp_to": null,             // 可选，有效期结束，毫秒时间戳
    "version": 1,                     // 可选，版本号，暂时固定为 1
    "token_limit": [                  // 权限列表
      // Limit 结构体，参考 Token 总览
      // 该字段为 Vec<_>，可指定多个
    ]
  }
}
```

TokenCreationRequest 结构体:

```rust
pub struct TokenCreationRequest {
    pub username: Option<String>,      // 可选，用户名
    pub password: Option<String>,      // 可选，密码
    pub timestamp_from: Option<i64>,   // 可选，有效期起始，毫秒时间戳
    pub timestamp_to: Option<i64>,     // 可选，有效期结束，毫秒时间戳
    pub version: Option<u8>,           // 可选，版本号，暂时固定为 1
    pub token_limit: Vec<Limit>,       // 权限列表
}
```

注意事项:

- 虽然 Username+Password 是可选字段，但必须同时存在或同时不存在
- Version 固定为 1（暂时）

### 权限要求

只有 **SuperToken** 可以创建 Token。

普通 Token 会返回权限错误。

### 返回值

返回值包含 `key` 与 `secret`，拼接后即可使用（格式: `key:secret`）:

```json
{
  "key": "n0kB8lSAykFd9Egu",                // Token Key
  "secret": "a0a7V3g43xjUCYIU5Md76H5QMPSlPPT6" // Token Secret
}
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "token_create",
  "params": {
    "father_token": "ROOT_KEY:ROOT_SECRET",
    "token_creation": {
      "username": "GM",
      "password": "ILoveRust1",
      "version": 1,
      "token_limit": [
        {
          "scopes": [
            "global"
          ],
          "permissions": [
            {
              "dynamic_monitoring": "write"
            },
            {
              "static_monitoring": "write"
            },
            {
              "task": "listen"
            }
          ]
        }
      ]
    }
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
    "key": "n0kB8lSAykFd9Egu",
    "secret": "a0a7V3g43xjUCYIU5Md76H5QMPSlPPT6"
  }
}
```

## Delete Token

删除指定的 Token。

### 方法

调用方法名为 `token_delete`，需要提供以下参数：

```json
{
  "token": "demo_super_token",                    // SuperToken
  "target_token": "target_token_key_or_username"   // 目标 Token 的 token_key 或 username
}
```

- `target_token` 为**必填**，不能为空字符串

`target_token` 支持两种匹配方式：

- `token_key`
- `username`

服务端会先按 `token_key` 匹配；若未命中，再按 `username` 匹配。

### 权限要求

只有 **SuperToken** 可以删除 Token。

普通 Token 会返回权限错误。

安全保护:

- **SuperToken 不可删除**
- 当 `target_token` 命中 SuperToken 的 `token_key` 或 `username` 时，服务端会拒绝请求并返回权限错误。

注意事项:

- 当 `target_token` 为空时，返回 `InvalidInput` 错误
- 当目标 Token 不存在时，返回 `NotFound` 错误

### 返回值

```json
{
  "message": "Token xxx deleted successfully by SuperToken", // 删除成功提示
  "rows_affected": 1,        // 受影响的行数
  "matched_by": "token_key"  // 匹配方式，token_key 或 username
}
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "token_delete",
  "params": {
    "token": "ROOT_KEY:ROOT_SECRET",
    "target_token": "n0kB8lSAykFd9Egu"
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
    "message": "Token n0kB8lSAykFd9Egu deleted successfully by SuperToken",
    "rows_affected": 1,
    "matched_by": "token_key"
  }
}
```

## Edit Token

修改指定 Token 的 `token_limit`。

### 方法

调用方法名为 `token_edit`，需要提供以下参数：

```json
{
  "token": "demo_super_token",                    // SuperToken
  "target_token": "target_token_key_or_username",  // 目标 Token 的 token_key 或 username
  "limit": [                                       // 新的权限列表，会覆盖原有 token_limit
    {
      "scopes": [
        "global"                                   // 全局作用域
      ],
      "permissions": [
        {
          "task": {
            "read": "ping"                         // 读取 ping 任务
          }
        }
      ]
    }
  ]
}
```

该方法会**覆盖**目标 Token 的 `token_limit` 字段。

`target_token` 支持两种匹配方式：

- `token_key`
- `username`

服务端会先按 `token_key` 匹配；若未命中，再按 `username` 匹配。

### 权限要求

只有 **SuperToken** 可以调用该方法。

普通 Token 会返回权限错误。

### 返回值

```json
{
  "success": true,                  // 是否成功
  "id": 2,                         // 目标 Token 在数据库中的 ID
  "token_key": "BgFqEhzoCISpAAON"  // 目标 Token 的 token_key
}
```

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "token_edit",
  "params": {
    "token": "ROOT_KEY:ROOT_SECRET",
    "target_token": "BgFqEhzoCISpAAON",
    "limit": [
      {
        "scopes": [
          "global"
        ],
        "permissions": [
          {
            "task": {
              "read": "ping"
            }
          },
          {
            "task": "listen"
          }
        ]
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
  "result": {
    "success": true,
    "id": 2,
    "token_key": "BgFqEhzoCISpAAON"
  }
}
```

## List All Tokens

列出数据库中的所有 Token 信息。

### 方法

调用方法名为 `token_list_all_tokens`，需要提供以下参数：

```json
{
  "token": "demo_super_token" // SuperToken
}
```

### 权限要求

只有 **SuperToken** 可以调用该方法。

普通 Token 会返回权限错误。

### 返回值

返回结构如下：

```json
{
  "tokens": [
    {
      "version": 1,                         // Token 版本
      "token_key": "n0kB8lSAykFd9Egu",      // Token Key
      "timestamp_from": null,               // 有效期起始
      "timestamp_to": null,                 // 有效期结束
      "token_limit": [],                    // 权限列表
      "username": "root"                    // 用户名，SuperToken 固定为 root
    },
    {
      "version": 1,
      "token_key": "demo_child_key",
      "timestamp_from": 1735689600000,      // 毫秒时间戳
      "timestamp_to": 1767225600000,
      "token_limit": [
        {
          "scopes": [
            "global"                        // 全局作用域
          ],
          "permissions": [
            {
              "task": "listen"              // 监听任务
            }
          ]
        }
      ],
      "username": "gm"
    }
  ]
}
```

注意事项:

`token_secret` 和 `password` 不会在该接口中返回。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "token_list_all_tokens",
  "params": {
    "token": "ROOT_KEY:ROOT_SECRET"
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
    "tokens": [
      {
        "version": 1,
        "token_key": "n0kB8lSAykFd9Egu",
        "timestamp_from": null,
        "timestamp_to": null,
        "token_limit": [],
        "username": "root"
      },
      {
        "version": 1,
        "token_key": "demo_child_key",
        "timestamp_from": 1735689600000,
        "timestamp_to": 1767225600000,
        "token_limit": [
          {
            "scopes": [
              "global"
            ],
            "permissions": [
              {
                "task": "listen"
              }
            ]
          }
        ],
        "username": "gm"
      }
    ]
  }
}
```

# 创建 Token

只有 SuperToken 有权限创建 Token

## 创建结构

```rust
pub struct TokenCreationRequest {
    pub username: Option<String>, // 可选
    pub password: Option<String>, // 可选

    pub timestamp_from: Option<i64>,
    pub timestamp_to: Option<i64>,

    pub version: Option<u8>,


    pub token_limit: Vec<Limit>,
}
```

虽然 Username+Password 是可选字段，但必须同时存在或同时不存在

Version 固定为 1 (暂时)

解析到 Json 如下:

```json
{
  "username": "GM",
  "password": "ILoveRust1",
  "version": 1,
  "token_limit": [
    // Token 总览中的 Limit 字段
    // 该字段为 Vec<_>，可指定多个
  ]
}
```

## 创建方法

`token_create` 是用于创建的方法，需要提供:

- `father_token`: 父 Token
- `token_creation`: 即上面结构体

```json
{
    "father_token": "demo_super_token",
    "token_creation": {
        // TokenCreationRequest 结构体
    }
}
```

## 返回值

返回值包含 `key` 与 `secret`，拼接后即可使用:

```json
{
    "key": "n0kB8lSAykFd9Egu",
    "secret": "a0a7V3g43xjUCYIU5Md76H5QMPSlPPT6"
}
```
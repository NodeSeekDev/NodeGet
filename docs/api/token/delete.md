# 删除 Token

删除指定的 Token。

## 方法

调用方法名为 `token_delete`，需要提供以下参数：

```json
{
  "token": "demo_super_token",
  "target_token": "target_token_key_or_username"
}
```

- `target_token` 为**必填**，不能为空字符串。

## 权限要求

只有 **SuperToken** 可以删除 Token。

`target_token` 支持两种匹配方式：

- `token_key`
- `username`

服务端会先按 `token_key` 匹配；若未命中，再按 `username` 匹配。

## 安全保护

- **SuperToken 不可删除**
- 当 `target_token` 命中 SuperToken 的 `token_key` 或 `username` 时，服务端会拒绝请求并返回权限错误。

## 错误语义

- 当 `target_token` 为空时，返回 `InvalidInput` 错误。
- 当目标 Token 不存在时，返回 `NotFound` 错误。
- 不再返回 `{ "success": false }` 这类业务失败响应。

## 成功返回

```json
{
  "message": "Token xxx deleted successfully by SuperToken",
  "rows_affected": 1,
  "matched_by": "token_key"
}
```

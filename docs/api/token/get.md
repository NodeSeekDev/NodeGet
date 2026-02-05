# 获取 Token 详情

提供一个 Token，即可获取在 `Token 总览` 中的 Token 对应结构体

## 获取方法

`token_get` 是用于获取的方法，需要提供:

- `token`: 需要查询的 Token

```json
{
    "token": "demo_token"
}
```

## 返回值

返回值即为 `Token 总览` 中的 Token 结构体:

```json
{
  "timestamp_from": null,
  "timestamp_to": null,
  "token_key": "n0kB8lSAykFd9Egu",
  "token_limit": [
    {
      "permissions": [
        {
          "task": "listen"
        },
        {
          "task": {
            "write": "ping"
          }
        },
        {
          "task": {
            "create": "ping"
          }
        },
        {
          "task": {
            "create": "tcp_ping"
          }
        }
      ],
      "scopes": [
        "global"
      ]
    }
  ],
  "username": null,
  "version": 1
}
```
# 获取 Metadata 详情

提供一个 Token 与 Agent Uuid，即可获取在指定 Server 中 Uuid 对应的 Metadata

## 获取方法

`metadata_get` 是用于获取的方法，需要提供:

- `token`: 具有对应权限的 Token
- `uuid`: 查询的 Uuid

```json
{
    "token": "demo_token",
    "uuid": "demo_uuid"
}
```

## 返回值

如下，基本类型为 `Metadata 总览` 中的结构体

```json
{
  "agent_name": "Agent-Name",
  "agent_tags": [
    "updated",
    "production",
    "web-server"
  ],
  "agent_uuid": "550e8400-e29b-41d4-a716-446655440000"
}
```

## 注意事项

若数据库中无查询的结果，返回为结构体，但各字段为空
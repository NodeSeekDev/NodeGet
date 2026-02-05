# 写入 Metadata 详情

提供一个 Token 与需要写入的 Metadata，即可完成创建 / 更新 Metadata

## 写入方法

`metadata_get` 是用于写入的方法，需要提供:

- `token`: 具有对应权限的 Token
- `metadata`: 写入的 Metadata，基本类型为 `Metadata 总览` 中的结构体

```json
{
  "token": "demo_token",
  "metadata": {
    "agent_uuid": "550e8400-e29b-41d4-a716-446655440000",
    "agent_name": "UpdatedAgentName",
    "agent_tags": [
      "updated",
      "production",
      "web-server"
    ]
  }
}
```

## 返回值

如下，`id` 为数据库中的 ID

```json
{
    "id": 1
}
```

## 注意事项

在单个 Server 中，Uuid 与 Metadata 的对应关系是唯一的，第二次写入同样的 Uuid 的 Metadata 视为更新数据
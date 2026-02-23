# 列出所有 Agent UUID

获取 Server 中所有 Agent 的 UUID 列表。

## 方法

调用方法名为 `nodeget-server_list_all_agent_uuid`，需要提供以下参数:

```json
{
  "token": "demo_token"
}
```

## 权限要求

- **Scope**: 必须为 `Global`，在 `AgentUuid` Scope 下无作用
- **Permission**: `NodeGet::ListAllAgentUuid`

## 返回结果

```json
{
  "uuids": [
    "e8583352-39e8-5a5b-b66c-e450689088fd",
    "a1b2c3d4-5e6f-7a8b-9c0d-1e2f3a4b5c6d"
  ]
}
```

## 数据来源

该方法会从以下三个表中获取所有不同的 Agent UUID：

1. `static_monitoring` - 静态监控数据表
2. `dynamic_monitoring` - 动态监控数据表
3. `task` - 任务数据表

返回的 UUID 列表是去重后按字母顺序排序的。
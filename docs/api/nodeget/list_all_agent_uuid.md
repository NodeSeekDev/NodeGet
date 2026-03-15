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

- **Permission**: `NodeGet::ListAllAgentUuid`
- **Scope 行为**:
    - `Global` Scope 下拥有该权限: 返回系统内所有 Agent UUID
    - `AgentUuid(xxx)` Scope 下拥有该权限: 可参与返回 `xxx`
    - 最终返回结果会再过滤为“当前 token 在该 `AgentUuid` 下至少有一种可操作权限（任一非 `NodeGet::ListAllAgentUuid` 权限）”的
      UUID

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

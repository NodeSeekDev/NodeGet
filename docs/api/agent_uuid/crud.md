# Agent UUID CRUD

## List All

列出当前系统中所有非软删除的 Agent UUID。

### 方法

调用方法名为 `agent-uuid_list_all`，需要提供以下参数：

```json
{
  "token": "demo_token"
  // 鉴权 Token
}
```

### 权限要求

- Permission: `MonitoringUuid::List`
- Scope: `Global`

### 返回值

返回 `Vec<Uuid>` 的 JSON 数组，每个元素为一个 UUID 字符串，按字母顺序排序。

```json
{
  "uuids" [
  "a1b2c3d4-5e6f-7a8b-9c0d-1e2f3a4b5c6d",
  "e8583352-39e8-5a5b-b66c-e450689088fd"
]
}
```

返回结果来源于 `monitoring_uuid_cache` 中的权威缓存，已过滤掉被软删除的记录。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "agent-uuid.list_all",
  "params": {
    "token": "demo_token"
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
    "e8583352-39e8-5a5b-b66c-e450689088fd",
    "a1b2c3d4-5e6f-7a8b-9c0d-1e2f3a4b5c6d"
  ]
}
```

### 行为说明

- **数据来源**：直接读取 **内存缓存**（`MonitoringUuidCache`），不涉及数据库查询，性能为 O(n) 纯内存遍历
- **过滤软删除**：仅返回 `soft_delete = false` 的活跃 Agent UUID，被软删除的记录不会出现在结果中
- **排序**：返回结果按 UUID 字母顺序排序，输出稳定
- **实时性**：数据为内存缓存的快照，仅在 `monitoring_uuid` 表发生变更并触发 `reload()` 后才会更新

---

## Delete

按 UUID 软删除指定的 Agent。

### 方法

调用方法名为 `agent-uuid_delete`，需要提供以下参数：

```json
{
  "token": "demo_token",
  // 鉴权 Token
  "agent_uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"
  // 要删除的 Agent UUID
}
```

### 权限要求

- Permission: `MonitoringUuid::Delete`
- Scope: `Global`

### 返回值

返回包含操作结果的对象：

```json
{
  "success": true,
  "message": "Agent UUID soft-deleted"
}
```

- `success`: `true` 表示成功软删除，`false` 表示该 UUID 不存在（或已被删除）
- `message`: 人类可读的操作结果描述

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "agent-uuid.delete",
  "params": {
    "token": "demo_token",
    "agent_uuid": "e8583352-39e8-5a5b-b66c-e450689088fd"
  },
  "id": 1
}
```

响应（成功）:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "success": true,
    "message": "Agent UUID soft-deleted"
  }
}
```

响应（UUID 不存在）:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "success": false,
    "message": "Agent UUID not found"
  }
}
```

### 行为说明

- 软删除仅设置 `monitoring_uuid` 表中的 `soft_delete` 字段为 `true`，不会物理删除任何数据
- 不会级联删除该 Agent 关联的监控数据、任务记录等历史数据
- 被软删除的 UUID 在后续 `agent-uuid.list_all` 调用中不再出现
- 若该 Agent 后续重新上报数据，其 UUID 可能再次出现在列表中（取决于具体业务逻辑）

---

## List All With Agent Mode

列出当前系统中所有 Agent UUID，包含每个 UUID 的软删除状态。

### 方法

调用方法名为 `agent-uuid_list_all_with_agent_mode`，需要提供以下参数：

```json
{
  "token": "demo_token"
  // 鉴权 Token
}
```

### 权限要求

- Permission: `MonitoringUuid::List`
- Scope: `Global`

### 返回值

返回 `Vec<{ uuid, soft_delete }>` 的 JSON 数组，按 UUID 字母顺序排序。

```json
[
  {
    "uuid": "a1b2c3d4-5e6f-7a8b-9c0d-1e2f3a4b5c6d",
    "soft_delete": false
  },
  {
    "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
    "soft_delete": true
  }
]
```

- `uuid`: Agent 的 UUID
- `soft_delete`: `true` 表示该 Agent 已被软删除，`false` 表示活跃状态

与 `agent-uuid.list_all` 的区别在于：此方法返回**所有** UUID（包括已软删除的），并明确标注每个 UUID 的状态。

### 完整示例

请求:

```json
{
  "jsonrpc": "2.0",
  "method": "agent-uuid_list_all_with_agent_mode",
  "params": {
    "token": "demo_token"
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
      "uuid": "a1b2c3d4-5e6f-7a8b-9c0d-1e2f3a4b5c6d",
      "soft_delete": false
    },
    {
      "uuid": "e8583352-39e8-5a5b-b66c-e450689088fd",
      "soft_delete": true
    }
  ]
}
```

### 行为说明

- **数据来源**：与 `agent-uuid_list_all` 一致，直接读取 **内存缓存**（`MonitoringUuidCache`），不涉及数据库查询，性能为 O(n)
  纯内存遍历
- **包含软删除记录**：返回所有已知的 Agent UUID，包括已被软删除的；通过 `soft_delete` 字段区分状态
- **排序**：返回结果按 UUID 字母顺序排序，输出稳定
- **实时性**：数据为内存缓存的快照，仅在 `monitoring_uuid` 表发生变更（如软删除、新增 Agent）并触发 `reload()` 后才会更新

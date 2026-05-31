# Agent UUID 总览

Agent UUID 是 NodeGet 中所有 Agent（客户端）的唯一标识。`agent-uuid` 模块提供面向前端的权威 Agent UUID 管理接口，数据来源于
`monitoring_uuid` 表（支持软删除）。

## 基本概念

- **权威数据源**：`monitoring_uuid` 表是 Agent UUID 的权威来源，Server 启动时会预热该缓存
- **软删除**：删除操作仅将 `soft_delete` 字段标记为 `true`，不会物理删除数据，也不影响关联的监控/任务历史记录
- **全局作用域**：当前所有 `agent-uuid` 操作均要求 `Global` Scope 权限

## 权限系统

`agent-uuid` 操作需要独立的 `MonitoringUuid` 权限（自 v0.3.0 起从 `NodeGet` 命名空间拆分）。

```rust
pub enum MonitoringUuid {
    List,    // 列出 Agent UUID
    Delete,  // 软删除 Agent UUID
}
```

### 权限配置示例

```json
{
  "scopes": [
    {"global": null}
  ],
  "permissions": [
    {"monitoring_uuid": "list"},
    {"monitoring_uuid": "delete"}
  ]
}
```

> `nodeget-server_list_all_agent_uuid` 已废弃，请迁移至 `agent-uuid` 命名空间。

## 方法列表

| 方法名                                                                       | 描述                      |
|---------------------------------------------------------------------------|-------------------------|
| [agent-uuid_list_all](./crud.md#list-all)                                 | 列出所有非软删除的 Agent UUID    |
| [agent-uuid_list_all_with_agent_mode](./crud.md#list-all-with-agent-mode) | 列出所有 Agent UUID（含软删除状态） |
| [agent-uuid_delete](./crud.md#delete)                                     | 按 UUID 软删除 Agent        |

# Metadata 总览

Metadata 也被称为 `元数据` / `Agent Name` 等

## 基本结构

目前，Metadata 只包含三个字段:

```rust
pub struct Metadata {
    pub agent_uuid: Uuid,
    pub agent_name: String,
    pub agent_tags: Vec<String>,
}
```

除了 Uuid 外，均可为空

## 注意事项

Metadata 只是对于 Server 而言的

对于不同的 Server，Agent 可能拥有不同的 Metadata 字段

Metadata 字段并非必须，在本项目相关内容非常之少，仅作为前端展示使用

Name、Uuid、Tags 均无查询、分组执行等功能
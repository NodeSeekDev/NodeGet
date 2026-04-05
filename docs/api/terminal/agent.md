---
outline: deep
---

# Agent Terminal 实现

实际上，官方 Agent 几乎全部源自 `GenshinMinecraft/komari-monitor-rs:src/callbacks/pty.rs`，Komari 提供的 WebShell 是较为成熟的方案，可以参考

## 任务处理

Agent 在处理 `web_shell` 任务时，会收到 `terminal_id` 字段。需要以该 ID 维护本地终端连接池，同一时刻不允许重复 ID

连接建立后，Agent 需要处理来自用户(而不是 Server) 发送的心跳包、Resize 请求，以及最重要的 Binary 数据

心跳包与 Resize 均通过文本类型的 WebSocket Message 发送

## 心跳包

心跳包结构如下:

```rust
struct HeartBeat {
    #[serde(rename = "type")]
    type_str: String,
    timestamp: String,
}
```

解析示例:

```json
{
    "type": "xx",
    "timestamp": 123
}
```

收到心跳包后，返回空值 (无数据) 即可

## Resize

Resize 用于调整终端大小，结构如下:

```rust
struct NeedResize {
    #[serde(rename = "type")]
    type_str: String,
    cols: u16,
    rows: u16,
}
```

解析示例:

```json
{
    "type": "xx",
    "cols": 114,
    "rows": 514
}
```

根据 `cols` 与 `rows` 通知 Pty 调整终端大小即可，返回空值 (无数据) 即可

## Binary 数据

Binary 类型数据直接发送到终端，无需二次处理

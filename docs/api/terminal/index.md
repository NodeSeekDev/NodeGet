# Terminal WebShell 总览

WebShell 是 Task 任务系统下的一个特殊功能，也叫 `网页 SSH` / `Terminal`

与 Monitoring 等基于 JsonRpc 的模块不同，Terminal 使用 WebSocket 连接进行双向通信

## 通信流程

Terminal 的通信流程如下:

1. 通过 `task_create_task` 创建一个 `web_shell` 类型的 Task，其中包含 `terminal_id` 字段
2. Agent 接收到任务后，主动通过 WebSocket 连接到 Server 提供的 Terminal Url
3. 用户(网页端) 通过 WebSocket 连接到 Server 提供的用户 Terminal Url
4. Server 在 Agent 与用户之间中继 Binary Message，实现双向通信

## 连接 URL

### Agent URL

由 NodeGet Server 提供的，Agent 连接的 Terminal Url 格式如下:

```
ws(s)://HOST(:PORT)/terminal?agent_uuid={agent_uuid}&task_id={task_id}&task_token={task_token}&terminal_id={terminal_id}
```

参数用于校验对应的 Task

该 Url 有以下两种生成方式:

- 以 `ws(s)://HOST(:PORT)/auto_gen` 为格式的 Url，将自动格式化成上述格式
- 用户指定 Url，可以是任意外部链接，包括但不限于其他监控 Server 提供的

### 用户 URL

由 NodeGet Server 提供的，用户连接的 Terminal Url 格式如下:

```
ws(s)://HOST(:PORT)/terminal?agent_uuid={agent_uuid}&terminal_id={terminal_id}&token=demo_token
```

用户在 Agent 连接后，可以与 Agent 进行双向 WebSocket 通信

## terminal_id 说明

`terminal_id` 必须与创建 `web_shell` Task 时提交的 `terminal_id` 一致

同一个 Agent 下，若本地已经存在相同 `terminal_id` 的终端连接，Agent 会拒绝新任务并上报任务错误，避免会话互相覆盖

## 权限要求

Terminal 功能需要对应 Agent 作用域下的 `Terminal::Connect` 权限

## 注意事项

实际上，该通道可以传输任意类型的 WebSocket 数据，包括但不限于心跳包、文本类型与 Binary 类型

后续会把该通道拓展使用方向，敬请期待

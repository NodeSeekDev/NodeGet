# Crontab 总览

Crontab 是本项目的定时任务功能，允许用户创建定时任务在指定时间自动执行特定操作。

## 基本概念

Crontab 允许用户创建定时任务，支持以下类型的任务：

- **Agent 任务**: 在特定 Agent 上执行的任务，如 ping、tcp_ping、http_ping、web_shell、execute、ip 等
- **Server 任务**: 在服务器端执行的任务，如数据库清理、触发 JsWorker 脚本等

## 基本结构

Crontab 任务的基本结构体:

```rust
pub struct Cron {
    pub id: i64,                    // 任务 ID
    pub name: String,               // 任务名称
    pub enable: bool,               // 是否启用
    pub cron_expression: String,    // Cron 表达式
    pub cron_type: CronType,        // 任务类型
    pub last_run_time: Option<i64>, // 最后运行时间
}

pub enum CronType {
    Agent(Vec<Uuid>, AgentCronType),  // Agent 类型任务
    Server(ServerCronType),           // Server 类型任务
}

pub enum AgentCronType {
    Task(TaskEventType),  // Agent 任务类型
}

pub enum ServerCronType {
    CleanUpDatabase,             // 数据库清理任务
    JsWorker(String, Value),     // 触发已注册 JsWorker（脚本名 + 入参）
}
```

### CronType 序列化示例

Agent 类型:

```json
{
    "agent": [
        [
            "00000000-0000-0000-0000-000000000001", // Agent UUID 列表
            "00000000-0000-0000-0000-000000000002"
        ],
        {
            "task": {
                "ping": "www.example.com" // AgentCronType::Task(TaskEventType)
            }
        }
    ]
}
```

Server 类型 (数据库清理):

```json
{
    "server": "clean_up_database"
}
```

Server 类型 (JsWorker):

```json
{
    "server": {
        "js_worker": [
            "demo_nodeget_fetch",  // 脚本名
            {
                "hello": "from_cron" // 传给脚本的 params（任意 JSON）
            }
        ]
    }
}
```

### ServerCronType::JsWorker 运行语义

- 每次触发会按 `run_type = cron` 调用脚本
- `params` 来自 Crontab 配置中的 `Value`
- 不从 Crontab 传 `env`，使用 `js_worker` 表内保存的 `env`
- 执行记录会写入 `js_result`，并在 `crontab_result.special_id` 中记录对应 `js_result.id`

## 权限系统

Crontab 操作需要特定的 Token 权限。在 Token 的权限限制 (Limit) 中，Crontab 相关权限定义如下：

```rust
pub enum Crontab {
    Read,    // 读取权限
    Write,   // 写入权限
    Delete,  // 删除权限
}
```

### 权限说明

- **Read**: 允许读取 Crontab 列表
- **Write**: 允许创建/修改/设置 Crontab 的启用状态
- **Delete**: 允许删除 Crontab

### 作用域限制

Crontab 权限遵循 Token 的作用域限制：

- **Global**: 具有全局权限的 Token 可以操作所有 Crontab
- **AgentUuid**: 具有特定 Agent UUID 权限的 Token 只能操作与这些 Agent 相关的 Agent 类型 Crontab

### 权限覆盖规则

对于写入类操作（编辑、删除、设置启用）：

- 服务端会先读取目标 Crontab 内容
- 再按该 Crontab 的 `cron_type` 展开的全部 Scope 做权限校验
- 必须完整覆盖所有 Scope 才允许操作

对于 `ServerCronType::JsWorker` 的创建/编辑，还要求：

- 具备 `Permission::JsWorker(JsWorker::RunDefinedJsWorker)`
- 且作用域覆盖 `Scope::JsWorker(script_name)`

## 方法列表

| 方法名 | 描述 |
|-------|------|
| [crontab_create](./crud.md#create-crontab) | 创建新的定时任务 |
| [crontab_edit](./crud.md#edit-crontab) | 修改已存在的定时任务 |
| [crontab_get](./crud.md#get-crontab) | 获取定时任务列表 |
| [crontab_delete](./crud.md#delete-crontab) | 删除指定的定时任务 |
| [crontab_set_enable](./crud.md#set-enable) | 控制定时任务的启用状态 |

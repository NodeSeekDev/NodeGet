# 特殊 Kv 与 特殊键

**本页面所有的 Namespace / K-V 都不会由 server 主动创建**

这里列出了一些 Kv 系统中的特殊 Namespace / 键名，是约定俗成的规范

## 特殊 Kv

- 每一个 Agent 的以自身 Uuid 为 Name 的 Namespace，称之为 `Agent Namespace`

- 每一个 Server 中以 `global` 为 Name 的 Namespace，称之为 `Server Namespace` 或直接称为 `global`

- 每一个 Server 中以 `frontend_[主题名]` 为 Name 的 Namespace，称之为 `Frontend Namespace`，是用于存放给主题所需要的配置文件的，比如官方的前端为
  `frontend_nodeget`

- 每一个 Token 的以自身 Token Key 为 Name 的 Namespace，称之为 `Token Namespace`

## 特殊键

在一个 Kv 中，非 Agent / Server 开发者不建议使用以下的键，其在 Agent / Server 内部有特殊用途，或为共同认定的功能键

### `Agent Namespace`

- `database_limit_*`:
    - `database_limit_static_monitoring`: 单位毫秒
      Crontab 执行 Server CleanUpDatabse 任务时，在 Static 表中查询最后一个该 Uuid 的数据，获取其 Timestamp

      清理 `从 (Timestamp - 该值) 至 Timestamp` **以外的**数据，可以理解为清理旧数据，保留新数据

      该设置不受数据条数影响，仅以 Timestamp 为标准

      若某一 Agent 设置了该值，并在历史某一时刻不再上传数据，则不会影响其 `从 (最后一个 Timestamp - 该值) 至 最后一个 Timestamp`
      的数据
    - `database_limit_dynamic_monitoring`: 同上，Dynamic Monitoring Data
    - `database_limit_task`: 同上，Task 记录
- `metadata_*`
    - `metadata_name`: 前端展示 Agent 名字
    - `metadata_tags`: 前端展示的 Tag，为数组，值为 String，如: `["tag1", "tag2"]`
    - `metadata_price`: 前端展示的机器价格
    - `metadata_price_unit`: 前端展示的机器单位
    - `metadata_price_cycle`: 前端展示的续费周期，单位天
    - `metadata_region`: 前端展示的地区代码，遵循 ISO 3166-1 二位字母代码 (仅作为展示，不代表 IP 地址所在地区)
    - `metadata_hidden`: 前端中隐藏，不代表没有权限访问

### `Server Namespace`

- `database_limit_crontab_result`: 与 `database_limit_*` 类似，Crontab 执行记录，但必须存在于 `global` Kv 中，其他位置无效

### Frontend Namespace

- `title`
- `description`
- `custom_body`
- `custom_head`

### Token Namespace

- `frontend_custom_body`
- `frontend_custom_head`

需要解释的是，Frontend 和 Token 的 Namespace 并不冲突。

Token Namespace 意为在使用这一 Token 登陆后，前端会使用的参数 (这里的 Token 应该是供给展示的)，前端可以进行拼接或优先使用
Token Namespace 定义的参数
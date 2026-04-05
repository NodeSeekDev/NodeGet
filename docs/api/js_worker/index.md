# JsWorker 总览

`JsWorker` 用于管理可复用的 JS 脚本，并在服务端运行这些脚本。

## 方法列表

| 方法名 | 描述 |
|---|---|
| [create](./crud.md#create-jsworker) | 创建脚本并预编译为字节码 |
| [read](./crud.md#read-jsworker) | 读取脚本详情 |
| [update](./crud.md#update-jsworker) | 更新脚本并重新预编译 |
| [delete](./crud.md#delete-jsworker) | 删除脚本 |
| [run](./crud.md#run-jsworker) | 异步运行已注册脚本，立即返回 `js_result` 记录 ID |
| [list_all_js_worker](./crud.md#list-all-jsworker) | 列出当前 Token 可见且存在的脚本名 |
| [get_rt_pool](./crud.md#get-runtime-pool) | 查看 JS Runtime 池状态 |

## 参考文档

| 文档 | 描述 |
|---|---|
| [script](./script.md) | JS 脚本编写规范与示例 |
| [injected](./injected.md) | JS Runtime 外部注入函数/对象清单 |
| [route](./route.md) | HTTP 路由绑定与 `onRoute` 处理说明 |

## 运行模型

`js-worker_run` 是异步模型：

1. 先写入 `js_result` 一条运行记录（含 `start_time/param`）
2. 立即返回这条记录的 `id`
3. 后台执行脚本
4. 执行结束后回填 `finish_time`，并写入 `result` 或 `error_message`

## 脚本入口

脚本必须 `export default` 一个对象，推荐至少实现：

- `onCall(params, env, ctx)`：用于 `run_type = "call"`
- `onCron(params, env, ctx)`：用于 `run_type = "cron"`

详细约束见 [script](./script.md)。

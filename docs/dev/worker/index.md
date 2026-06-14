# Worker 开发

NodeGet 使用 [QuickJS](https://bellard.org/quickjs/) 作为 JS Worker 的运行时。每个 Worker 本质上是一个 ES 模块，通过 `export default` 导出一个包含事件处理函数的对象。

## 事件处理函数

```js
export default {
  async onCall(params, env, ctx) {},
  async onCron(params, env, ctx) {},
  async onRoute(request, env, ctx) {},
  async onInlineCall(params, env, ctx) {},
};
```

- `onCall`：手动调用或 Cron 触发时执行。
- `onCron`：Cron 任务触发时执行。
- `onRoute`：HTTP 路由请求时执行，必须返回 `Response`。
- `onInlineCall`：被其他 Worker 通过 `inlineCall()` 调用时执行。

## 内置全局 API

Worker 内可使用以下全局对象/函数：

- `nodeget(method, params)`：发起 JSON-RPC 调用。
- `fetch(...)`：HTTP 请求。
- `execSql(token, sql, params?)`：执行 SQL。
- `db.*`：数据库 CRUD 快捷方式。
- `randomUUID()`：生成 UUID。
- `inlineCall(name, params, timeoutSec?)`：内联调用其他 Worker。
- `nodegetLog.*`：向 Server `tracing` 输出日志。

## 相关文档

- [JS Worker 日志](./logging.md)
- [扩展开发](../extension/index.md)
- [Bootstrap 说明](../bootstrap/index.md)

# NodeGet Worker 的架构

NodeGet Worker = `QuickJS` + `llrt library` + `NodeGet extension / bind`

NodeGet Worker 是 NodeGet 嵌入的 JS 运行时（Runtime），其基础来自于 QuickJS 项目。

选择 QuickJS 而非 V8 引擎的原因是：QuickJS 具有快速的启动时间和极小的内存足迹和打包体积，非常适合作为探针的嵌入式运行时。

同时为了在 Js Worker 内方便调用 NodeGet 项目的接口，提供了名为 `nodeget` 的全局函数。

为了支持标准 JS 语法之外的扩展功能，借助了 AWS 的 llrt 项目，补充实现了下面的模块：

- `fetch`/`Request`/`Response`/`Headers`/`FormData`
- `atob`/`btoa`/`TextEncoder`/`TextDecoder`
- `URL`/`URLSearchParams`
- `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval`/`setImmediate`/`queueMicrotask`
- `Buffer`/`Blob`/`File`
- `ReadableStream`/`WritableStream`/`TransformStream`
- `util` 模块

关于 NodeGet Worker 的详细能力扩展说明，可以参考 [API](/api/js_worker/injected)。

此外，我们扩展了更多的调用入口，比如 `onCall` / `onCron` / `onRoute` / `onInlineCall`，使其能够和 NodeGet 项目无缝结合。

```
export default {
  // 通过 JSON-RPC 调用
  async onCall(params, env, ctx) {
    return { ok: true, from: "onCall", params, env };
  },

  // 通过 Worker 相互调用
  async onInlineCall(params, env, ctx) {
    return { ok: true, from: "onInlineCall", params, env };
  },

  // 通过 Cron、JSON-RPC 调用
  async onCron(params, env, ctx) {
    return { ok: true, from: "onCron", params, env };
  },

  // 通过 HTTP 请求、JSON-RPC 调用
  async onRoute(request, env, ctx) {
    return new Response("ok", { status: 200 });
  }
};
```

这些特殊函数如何工作，可以参考 [代码规范](./coding-guide) 和 [API](/api/js_worker/script)。

## Worker 的调用关系

下面简单介绍下 Worker 支持的调用关系。

### API 调用 Worker

通过 JSON-RPC 发起 `js-worker_run`，可以执行 `onCall` 函数，为了方便开发调试，这个函数也支持模拟触发 `onCron`、`onRoute`。

并提供相关的 `params` 变量。

### Worker 调用 API

可以通过 Worker 内的 `nodeget` 函数，调用所有的 NodeGet 接口，这个是跳过了 WebSocket 请求，直接触发对应的行为逻辑。

虽然是没有网络数据包，但所需要的鉴权 Token 并不会跳过，对于不同的 API，仍然需要提供所需的 Token。

Worker 只是行为，不代表权限。

### Worker 调用 Worker

Worker 之间可以通过 `inlineCall` 入口相互调用，可以通过 `ctx.inlineCaller` 获得调用者并决定是否继续执行。

### HTTP 路由绑定

Worker 可以绑定 HTTP 路由，进而实现与外部系统交互，比如各种 Webhook，具体的应用如：

- Telegram 机器人可以完整部署到 NodeGet Worker 上
- GitHub 更新的 Webhook

## Worker 的运行模式

下面是 NodeGet 在实现 Worker 时采取的一些考量：

- 每个 Worker 的代码被更新时会预编译为字节码提高运行效率
- 一个 Worker 对应一个 Runtime 实例，Worker 间相互隔离
- Runtime 长时间不使用会被清理，可以设定每个 Runtime 的不活跃清理时间
- 每个 Worker 储存它们自己的 `env` 变量，在函数运行时会被注入
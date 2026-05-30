# JS 脚本编写规范

`js-worker` 的脚本应使用 ES Module 形式，`export default` 一个对象。

## 必要导出

建议始终导出：

```js
export default {
  async onCall(params, env, ctx) {
    return { ok: true, from: "onCall", params, env };
  },

  async onInlineCall(params, env, ctx) {
    return { ok: true, from: "onInlineCall", params, env };
  },

  async onCron(params, env, ctx) {
    return { ok: true, from: "onCron", params, env };
  },

  async onRoute(request, env, ctx) {
    return new Response("ok", { status: 200 });
  }
};
```

运行时根据 `run_type` 调用：

- `call` -> `export default.onCall(...)`
- `inline_call` -> `export default.onInlineCall(...)`
- `cron` -> `export default.onCron(...)`
- `route` -> `export default.onRoute(...)`

## 参数约定

`onCall` / `onInlineCall` / `onCron` 入口签名：

```js
async function handler(params, env, ctx) {}
```

- `params`：来自 `js-worker_run` 的 `params`
- `env`：来自 `js-worker_run.env` 或数据库保存的 `env`
- `ctx`：运行时上下文，当前包含：
    - `ctx.runType`：当前入口名（`"onCall"` / `"onInlineCall"` / `"onCron"` / `"onRoute"`）
    - `ctx.workerName`：当前 Worker 的名字
    - `ctx.inlineCall(js_worker_name, params, timeout_sec?)`：调用另一个 JS Worker 的 `onInlineCall`，返回其结果；会写入
      `js_result`。`timeout_sec` 为可选软超时（秒，正有限数），最终生效超时取 `timeout_sec` 与目标 Worker `max_run_time`
      中较小者。
    - `ctx.inlineCaller`：调用者脚本名（如 A 通过 `inlineCall` 调 B，则 B 中该值为 `"A"`；顶层调用为 `null`）

`onRoute` 入口签名：

```js
async function onRoute(request, env, ctx) {}
```

- `request`：运行时直接传入的 Fetch 标准 `Request` 对象
  可通过 `request.headers.get("NG-Connecting-IP")` 获取 TCP 对端 IP（头名大小写不敏感）。
- `env`：来自数据库保存的 `env`
- `ctx`：与其他入口一致

## per-Worker 运行限制

`js_worker` 表中提供以下可选字段，用于控制单个 Worker 的运行资源。未设置时使用默认值。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `max_run_time` | `i64` (ms) | `30000`（30 秒） | 单次执行总时长硬上限（毫秒），超时后运行时自动终止 |
| `max_stack_size` | `i64` (bytes) | `1048576`（1 MiB） | QuickJS C 栈上限（字节），防止栈溢出 |
| `max_heap_size` | `i64` (bytes) | `8388608`（8 MiB） | QuickJS 堆内存上限（字节），超限时分配失败 |

这些字段在创建/更新 Worker 时通过 `js-worker_create` / `js-worker_update` 设置，设为 `null` 或不传则使用上方默认值。

`inlineCall` 的软超时 `timeout_sec` 会与目标 Worker 的 `max_run_time` 取较小者作为最终生效超时。

## 返回值约束

- 必须返回可 JSON 序列化的数据（对象/数组/字符串/数字/布尔/null）。
- 不允许返回 `undefined`。
- `onRoute` 必须返回 `Response` 对象。

## 可用能力

- `fetch`：已注入，可直接发 HTTP 请求。
- `globalThis.nodeget(...)`：已注入，支持三种调用方式：
    - `nodeget(json)` — 传入完整 JSON-RPC 请求（string 或 object）
    - `nodeget(method, params)` — 快捷方式，自动生成 id
    - `nodeget(method, params, id)` — 快捷方式，指定 id
      返回解析后的 JS 对象。
- `globalThis.randomUUID()`：已注入，生成随机 UUID v4 字符串。
- `ctx.inlineCall`：已注入，可 `await` 调用指定 `js_worker` 的 `onInlineCall`。
- 更多注入函数/对象见 [injected](./injected.md)。

## 推荐示例（同时使用 nodeget + fetch）

```js
export default {
  async onCall(params, env, ctx) {
    // 快捷方式：nodeget(method, params)
    const hello = await nodeget("nodeget-server_hello", []);

    const resp = await fetch("https://httpbin.org/get");
    const text = await resp.text();

    return {
      ok: true,
      hello: hello.result,
      fetch_status: resp.status,
      body_preview: text.slice(0, 120),
      params,
      env
    };
  },

  async onCron(params, env, ctx) {
    return { ok: true, from: "cron", params, env };
  },

  async onRoute(request, env, ctx) {
    const text = await request.text();
    return new Response(
      JSON.stringify({
        ok: true,
        method: request.method,
        url: request.url,
        text,
        env
      }),
      {
        status: 200,
        headers: { "content-type": "application/json; charset=utf-8" }
      }
    );
  }
};
```

## 提交脚本时的编码

- `js-worker_create` / `js-worker_update` 传的是 `js_script_base64`。
- Base64 原文必须是 UTF-8 编码的 JS 源码。

## 预编译说明

- 创建/更新时会进行"仅编译"预检查，不会执行业务逻辑。
- 真正执行发生在 `js-worker_run`。
- HTTP 路由调用发生在 `/nodeget/worker-route/{route_name}`（旧 `/worker-route/{route_name}` 仍兼容，后续版本将移除）。

# JS 脚本编写规范

`js-worker` 的脚本应使用 ES Module 形式，`export default` 一个对象。

## 必要导出

建议始终导出：

```js
export default {
  async onCall(params, env, ctx) {
    return { ok: true, from: "onCall", params, env };
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
- `cron` -> `export default.onCron(...)`
- `route` -> `export default.onRoute(...)`

## 参数约定

`onCall` / `onCron` 入口签名：

```js
async function handler(params, env, ctx) {}
```

- `params`：来自 `js-worker_run` 的 `params`
- `env`：来自 `js-worker_run.env` 或数据库保存的 `env`
- `ctx`：运行时上下文，当前包含：
  - `ctx.nodeget(rawJsonString)`：调用 Server 内部 JSON-RPC
  - `ctx.uuid()`：生成随机 UUID v4 字符串
  - `ctx.runType`：当前入口名（`onCall` / `onCron` / `onRoute`）

`onRoute` 入口签名：

```js
async function onRoute(request, env, ctx) {}
```

- `request`：运行时直接传入的 Fetch 标准 `Request` 对象
- `env`：来自数据库保存的 `env`
- `ctx`：与其他入口一致

## 返回值约束

- 必须返回可 JSON 序列化的数据（对象/数组/字符串/数字/布尔/null）。
- 不允许返回 `undefined`。
- `onRoute` 必须返回 `Response` 对象。

## 可用能力

- `fetch`：已注入，可直接发 HTTP 请求。
- `ctx.nodeget`：已注入，参数是 JSON 字符串，返回也是 JSON 字符串。
- 更多注入函数/对象见 [injected](./injected.md)。

## 推荐示例（同时使用 nodeget + fetch）

```js
export default {
  async onCall(params, env, ctx) {
    const helloRaw = await ctx.nodeget(JSON.stringify({
      jsonrpc: "2.0",
      method: "nodeget-server_hello",
      params: [],
      id: 1001
    }));
    const hello = JSON.parse(helloRaw);

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

- 创建/更新时会进行“仅编译”预检查，不会执行业务逻辑。
- 真正执行发生在 `js-worker_run`。
- HTTP 路由调用发生在 `/worker-route/{route_name}`。

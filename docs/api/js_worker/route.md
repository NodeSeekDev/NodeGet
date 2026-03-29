# HTTP Route 绑定

当 `js_worker.route_name` 不为 `null` 时，该脚本会启用 HTTP 路由入口。

路由路径：

- `http://{host}/worker-route/{route_name}`
- `http://{host}/worker-route/{route_name}/*`

## 启用方式

在 `js-worker_create` 或 `js-worker_update` 里传入 `route_name`。

约束：

- `route_name` 必须唯一（数据库唯一索引）
- 只允许字符：`a-z A-Z 0-9 . _ -`

## 脚本入口

开启路由后，脚本需要实现：

```js
export default {
  async onRoute(request, env, ctx) {
    return new Response("ok", { status: 200 });
  }
};
```

- `request` 为 Fetch 标准 `Request` 对象
- 返回值必须是 Fetch 标准 `Response` 对象

## 示例

脚本：

```js
export default {
  async onRoute(request, env, ctx) {
    return new Response(
      JSON.stringify({
        ok: true,
        method: request.method,
        path: new URL(request.url).pathname,
        trace_id: ctx.uuid()
      }),
      {
        status: 200,
        headers: { "content-type": "application/json; charset=utf-8" }
      }
    );
  }
};
```

请求：

```bash
curl -i http://127.0.0.1:3000/worker-route/demo_route/hello
```

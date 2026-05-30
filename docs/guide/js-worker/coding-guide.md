# 代码规范

## 示例代码

仍然以这个代码举例：

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

## 参数规范

这些函数被调用时会遵守下面的规范注入相关参数：

- `ctx`：执行时的上下文，里面含有当前 Worker 的名称、运行类型、`inlineCall` 的调用者
- `params`：API、Cron 调用时用户提供的变量
- `env`：由用户设定的每个 Worker 的 `env` 变量，长期储存在数据库中
- `request`：标准的 JS `Request` 对象

具体规定可以参考 [API](/api/js_worker/) 章节。

## 环境变量

虽然目前环境变量可以存储任意的 JSON 数据，但为了规范起见，建议仅储存字符串 => 字符串的 Key-Value 映射，例如：

```
{
    "mode":"production"
}
```

目前 Dashboard 是这种情况专门设计的，减少一定的自由度来提供统一的工程规范。

## Worker 描述

允许每个 Worker 设定自己的描述属性，该属性会以 Markdown 文本的方式渲染，开发者应该在此说明：

- `onCall` 调用接口的参数（`params`）
- HTTP 路由及参数
- 定时任务的要求
- 环境变量及含义

## 工程化

NodeGet Worker 基于 ES Module 运行，支持 `export default` 语法，但暂不支持 `import` 语句加载外部模块。如果需要（多文件）模块化机制，可以在本地项目打包。

推荐使用 `esbuild` 打包，建议不要开启最小化，保持打包后代码的可读性。

提供一个可以参考的配置：

```json
{
    bundle: true,
    format: "esm",
    minify: false,
    platform: "browser",
    target: "es2022"
    // sourcemap: true
}
```

可以把 [Bootstrap](https://github.com/NodeSeekDev/NodeGet-Bootstrap) 的代码作为一个简易的例子。
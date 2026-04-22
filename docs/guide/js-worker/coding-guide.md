# 代码规范

## 示例代码

仍然以这个代码举例

```
export default {
  // 通过 JSONRpc调用
  async onCall(params, env, ctx) {
    return { ok: true, from: "onCall", params, env };
  },

  // 通过 worker 相互调用
  async onInlineCall(params, env, ctx) {
    return { ok: true, from: "onInlineCall", params, env };
  },

  // 通过 cron、 JSONRpc调用
  async onCron(params, env, ctx) {
    return { ok: true, from: "onCron", params, env };
  },

  // 通过 http请求、 JSONRpc调用
  async onRoute(request, env, ctx) {
    return new Response("ok", { status: 200 });
  }
};
```

## 参数规范

这些函数被调用时会遵守下面的规范注入相关参数

- ctx: 执行时的上下文，里面含有当前 worker的名称，运行类型，inlineCall的调用者
- param: api、cron调用时用户提供的变量
- env: 由用户设定的每个worker的env变量，产期储存在数据库中
- request: 标准的js Request对象

具体规定可以参考[API](/api/js_worker/)章节

## 环境变量

虽然目前环境变量可以存储任意的JSON数据，但为了规范起见，建议仅储存 字符串 => 字符串的 key-value映射，例如
```
{
    "mode":"production"
}
```

目前 dashboard 是这种情况专门设计的，减少一定的自由度来提供统一的工程规范。

## worker 描述

允许每个worker设定自己的描述属性，该属性会以markdown文本的方式渲染，开发者应该在此说明
- onCall调用接口的参数（params）
- http路由及参数
- 定时任务的要求
- 环境变量及含义

## 工程化

NodeGet worker暂时并不提供 import 支持，如果需要（多文件）模块化机制，可以在本地项目打包

推荐使用esbuild打包，建议不要开启最小化，保持打包后代码的可读性。

提供一个可以参考的配置

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

可以把 [bootstrap](https://github.com/NodeSeekDev/NodeGet-Bootstrap) 的代码作为一个简易的例子
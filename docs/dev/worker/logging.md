# JS Worker 日志

NodeGet 的 JS Worker 运行环境内置了 `nodegetLog` 全局对象，用于把 Worker 内部的日志直接桥接到 Server 端的 Rust [
`tracing`](https://github.com/tokio-rs/tracing) 日志系统。

它**不是**浏览器或 Node.js 的 `console`，不提供格式化占位符、循环引用保护、断言、计时器等高级语义；只有纯粹的“按级别 +
按命名空间输出一段文本”。

## 可用的日志级别

`nodegetLog` 提供以下方法，对应 `tracing` 的五个级别：

| JS 方法                                  | tracing 级别 | 用途         |
|----------------------------------------|------------|------------|
| `nodegetLog.trace(namespace, message)` | `TRACE`    | 最详细的调试信息   |
| `nodegetLog.debug(namespace, message)` | `DEBUG`    | 调试信息       |
| `nodegetLog.info(namespace, message)`  | `INFO`     | 普通信息       |
| `nodegetLog.warn(namespace, message)`  | `WARN`     | 警告         |
| `nodegetLog.error(namespace, message)` | `ERROR`    | 错误         |
| `nodegetLog.log(namespace, message)`   | `INFO`     | `info` 的别名 |

## 参数说明

- `namespace`：日志的独立空间/分类，任意字符串。空字符串、`null`、`undefined` 都会落到默认命名空间 `default`。
- `message`：日志正文，会强制 `String()` 转换。如果要记录对象，请先在 JS 侧 `JSON.stringify()`。

## 示例

```js
export default {
  async onCall(params, env, ctx) {
    nodegetLog.info("lifecycle", "onCall start");

    try {
      const result = await nodeget("kv_get_value", {
        token: env.token,
        namespace: "global",
        key: "inited",
      });
      nodegetLog.debug("kv", `inited=${JSON.stringify(result.result)}`);
    } catch (e) {
      nodegetLog.error("kv", `read failed: ${e}`);
    }

    return { ok: true };
  },
};
```

## 在 Server 终端查看日志

`nodegetLog` 的输出 target 固定为 `js_worker`，并携带 `worker`（当前脚本名）与 `namespace` 两个结构化字段。默认终端输出类似：

```text
2026-06-14T15:00:00.123456Z  INFO js_worker: onCall start worker="my-worker" namespace="lifecycle"
2026-06-14T15:00:00.234567Z DEBUG js_worker: inited=true worker="my-worker" namespace="kv"
```

启动 Server 时若要打开 `debug`/`trace` 级别的 Worker 日志，需要显式设置 `RUST_LOG`，例如：

```bash
RUST_LOG=info,js_worker=debug cargo run --package nodeget-server -- serve -c config.toml
```

若要将 Worker 的 `trace` 级别也输出出来，则改为 `js_worker=trace`。

## 设计说明

- `nodegetLog` 在每次创建 QuickJS 上下文时注入，因此同时覆盖：
    - 持久化 Worker 池（`js_worker` 表注册脚本的常规执行）
    - 一次性执行路径（inline call、source mode 等）
- Worker 名来自执行前设置的 `__nodeget_current_script_name`。如果在模块加载阶段、尚未进入 handler 时调用，Worker 名会显示为
  `unknown`。
- 日志同步桥接到 `tracing`，没有额外的异步队列或缓存，不会被 Worker 执行完成后的 GC/清理截断。

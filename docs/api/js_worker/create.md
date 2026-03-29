# 创建脚本

调用者可以通过 `js-worker_create` 创建脚本。

需要传入 `token` / `name` / `js_script_base64` / `route_name` / `runtime_clean_time` / `env`：

```json
{
  "token": "demo_token",
  "name": "demo_worker",
  "js_script_base64": "ZXhwb3J0IGRlZmF1bHQgeyBhc3luYyBvbkNhbGwocGFyYW1zLCBlbnYsIGN0eCkgeyByZXR1cm4geyBvazogdHJ1ZSB9OyB9IH07",
  "route_name": "demo_route",
  "runtime_clean_time": 60000,
  "env": {
    "region": "ap-east-1"
  }
}
```

参数说明：

- `name`：脚本唯一名称。
- `js_script_base64`：Base64 编码后的 UTF-8 JS 源码。
- `route_name`：可选。若设置则开启 HTTP 路由入口，对应路径前缀为 `/worker-route/{route_name}`。
- `runtime_clean_time`：脚本 Runtime 空闲清理时间（毫秒），`null` 表示不自动清理。
- `env`：可选，任意 JSON 结构，存入数据库并可在运行时传给脚本。

返回结构：

```json
{
  "id": 1,
  "name": "demo_worker",
  "route_name": "demo_route",
  "create_at": 1774652000123,
  "update_at": 1774652000123
}
```

## 完整示例

```json
{
  "jsonrpc": "2.0",
  "method": "js-worker_create",
  "params": {
    "token": "demo_token",
    "name": "demo_worker",
    "js_script_base64": "ZXhwb3J0IGRlZmF1bHQgeyBhc3luYyBvbkNhbGwocGFyYW1zLCBlbnYsIGN0eCkgeyByZXR1cm4geyBvazogdHJ1ZSwgcGFyYW1zLCBlbnYgfTsgfSwgYXN5bmMgb25Dcm9uKHBhcmFtcywgZW52LCBjdHgpIHsgcmV0dXJuIHsgb2s6IHRydWUsIGNyb246IHRydWUgfTsgfSB9Ow==",
    "route_name": "demo_route",
    "runtime_clean_time": 60000,
    "env": {
      "project": "NodeGet"
    }
  },
  "id": 1
}
```

## 权限要求

- 需要 `Permission::JsWorker(JsWorker::Create)`。
- 作用域要求：`Scope::JsWorker(name)` 必须覆盖目标脚本名。
  支持后缀 `*` 通配（如 `test_*`）。

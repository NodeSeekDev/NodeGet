# 更新脚本

调用者可以通过 `js-worker_update` 更新脚本。

需要传入 `token` / `name` / `js_script_base64` / `route_name` / `runtime_clean_time` / `env`：

```json
{
  "token": "demo_token",
  "name": "demo_worker",
  "js_script_base64": "ZXhwb3J0IGRlZmF1bHQgeyBhc3luYyBvbkNhbGwocGFyYW1zLCBlbnYsIGN0eCkgeyByZXR1cm4geyBvazogdHJ1ZSwgdmVyc2lvbjogMiB9OyB9IH07",
  "route_name": "demo_route_v2",
  "runtime_clean_time": 120000,
  "env": {
    "region": "ap-southeast-1"
  }
}
```

返回结构：

```json
{
  "success": true,
  "name": "demo_worker",
  "route_name": "demo_route_v2",
  "update_at": 1774652666000
}
```

说明：

- 更新后会重新预编译字节码。
- 已存在的 Runtime 实例会被立即驱逐，后续运行会使用新版本脚本。
- `route_name = null` 可关闭该脚本的 HTTP 路由绑定。

## 完整示例

```json
{
  "jsonrpc": "2.0",
  "method": "js-worker_update",
  "params": {
    "token": "demo_token",
    "name": "demo_worker",
    "js_script_base64": "ZXhwb3J0IGRlZmF1bHQgeyBhc3luYyBvbkNhbGwocGFyYW1zLCBlbnYsIGN0eCkgeyByZXR1cm4geyBvazogdHJ1ZSwgdmVyc2lvbjogMiB9OyB9IH07",
    "route_name": "demo_route_v2",
    "runtime_clean_time": 120000,
    "env": {
      "project": "NodeGet"
    }
  },
  "id": 1
}
```

## 权限要求

- 需要 `Permission::JsWorker(JsWorker::Write)`。
- 作用域要求：`Scope::JsWorker(name)`。

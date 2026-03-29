# 读取脚本

调用者可以通过 `js-worker_read` 读取脚本信息。

需要传入 `token` / `name`：

```json
{
  "token": "demo_token",
  "name": "demo_worker"
}
```

返回结构：

```json
{
  "name": "demo_worker",
  "route_name": "demo_route",
  "js_script_base64": "ZXhwb3J0IGRlZmF1bHQgeyBhc3luYyBvbkNhbGwocGFyYW1zLCBlbnYsIGN0eCkgeyByZXR1cm4geyBvazogdHJ1ZSB9OyB9IH07",
  "runtime_clean_time": 60000,
  "env": {
    "region": "ap-east-1"
  },
  "create_at": 1774652000123,
  "update_at": 1774652000123
}
```

## 完整示例

```json
{
  "jsonrpc": "2.0",
  "method": "js-worker_read",
  "params": {
    "token": "demo_token",
    "name": "demo_worker"
  },
  "id": 1
}
```

## 权限要求

- 需要 `Permission::JsWorker(JsWorker::Read)`。
- 作用域要求：`Scope::JsWorker(name)`。

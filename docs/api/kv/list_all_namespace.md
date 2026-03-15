# 列出所有命名空间

列出当前 Server 中可访问的 Kv Namespace。

## 方法

调用方法名为 `kv_list_all_namespace`，需要提供以下参数：

```json
{
  "token": "demo_token"
}
```

## 权限要求

- Permission: `Kv::ListAllNamespace`
- Scope 规则:
    - 在 `Global` Scope 下拥有该权限: 可以列出所有 Namespace
    - 在 `KvNamespace(xxx)` Scope 下拥有该权限: 仅能列出该 Scope 对应的 Namespace
    - 未拥有该权限: 返回权限错误

## 返回结果

返回一个字符串数组，每个元素是一个 Namespace 名称：

```json
[
  "global",
  "frontend_nodeget",
  "830cec66-8fc9-5c21-9e2d-2da2b2f2d3b3"
]
```


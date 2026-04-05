# NodeGet 总览

NodeGet 是本项目的基础服务接口模块，提供服务端状态查询、版本信息获取、配置管理等功能

所有方法均位于 `nodeget-server` 命名空间下

## 方法列表

| 方法名 | 描述 | 权限要求 |
|-------|------|---------|
| [hello](./crud.md#hello) | 测试服务是否正常运行 | 无 |
| [version](./crud.md#version) | 获取服务端版本信息 | 无 |
| [uuid](./crud.md#uuid) | 获取当前 Server UUID | 无 |
| [list_all_agent_uuid](./crud.md#list-all-agent-uuid) | 获取所有 Agent UUID 列表 | `NodeGet::ListAllAgentUuid` |
| [read_config](./crud.md#read-config) | 读取服务端配置文件原文 | SuperToken |
| [edit_config](./crud.md#edit-config) | 写入并触发服务端配置热重载 | SuperToken |

## 版本信息结构体

调用 `nodeget-server_version` 返回的 `NodeGetVersion` 结构如下:

```json
{
    "binary_type": "Server", // 二进制类型
    "build_time": "2026-02-08T10:44:02.848471700Z", // 构建时间
    "cargo_target_triple": "x86_64-pc-windows-msvc", // 编译目标
    "cargo_version": "0.0.1", // Cargo 版本号
    "git_branch": "main", // Git 分支
    "git_commit_date": "2026-02-08T07:25:09.000000000Z", // 提交日期
    "git_commit_message": "Feat: ...", // 提交信息
    "git_commit_sha": "73d9589", // 提交 SHA
    "rustc_channel": "nightly", // Rust 编译器通道
    "rustc_commit_date": "2025-12-30", // Rust 编译器提交日期
    "rustc_commit_hash": "0e8999942552691afc20495af6227eca8ab0af05", // Rust 编译器提交 Hash
    "rustc_llvm_version": "21.1", // LLVM 版本
    "rustc_version": "1.94.0-nightly" // Rust 版本
}
```

## Agent UUID 列表结构体

调用 `nodeget-server_list_all_agent_uuid` 返回的结构如下:

```json
{
    "uuids": [
        "e8583352-39e8-5a5b-b66c-e450689088fd",
        "a1b2c3d4-5e6f-7a8b-9c0d-1e2f3a4b5c6d"
    ]
}
```

该方法从以下三个表中获取所有不同的 Agent UUID:

1. `static_monitoring` - 静态监控数据表
2. `dynamic_monitoring` - 动态监控数据表
3. `task` - 任务数据表

返回的 UUID 列表是去重后按字母顺序排序的

## 注意事项

`hello` / `version` / `uuid` 三个方法不需要任何鉴权，可直接调用

`list_all_agent_uuid` 需要 Token 拥有 `NodeGet::ListAllAgentUuid` 权限，返回结果受 Scope 限制

`read_config` / `edit_config` 仅允许 **SuperToken** 调用，`token` 支持 `token_key:token_secret` 或 `username|password` 两种格式

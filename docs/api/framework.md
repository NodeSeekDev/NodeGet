---
outline: deep
---

# 架构概述

该项目使用 Rust 作为开发语言，若你需要进行二次开发，请务必熟读该文档。

- `Agent` / `探针端` / `客户端` 等均指代 `nodeget-agent`
- `Server` / `服务端` / `主控` 等均指代 `nodeget-server`
- `调用者` / `前端` / `第三方项目` 等均指代使用 `nodeget-server` 提供的 JSON-RPC API 进行处理、展示、使用的项目

## 总览

本项目分为三个部分:

- nodeget-agent: 监控 Agent
- nodeget-server: 服务端，提供 API
- nodeget-lib: 用于存放公共结构体、以及一些 utils 代码

目前还是传统 Client / Server 架构

## 基本亮点

- 细粒度权限支持，可以通过规范权限 Token 以便于第三方集成
- Powered By Rust，server / agent 性能优秀，系统资源占用低
- 活跃的开发团队
- 前后端分离
- ...

## 通信协议

推荐阅读: <https://wiki.geekdream.com/Specification/json-rpc_2.0.html>

`nodeget-server` 提供了一个 WebSocket JSON-RPC 服务器，并在同端口同样提供 HTTP POST JSON-RPC 服务器，除无法进行长连接外与
WebSocket JSON-RPC 无异。

推荐使用 JSON-RPC 进行二次开发时同时兼容 WebSocket 与 HTTP，并优先使用 WebSocket 通信。

在非 Windows 平台可选启用 Unix Socket 监听（`enable_unix_socket` / `unix_socket_path`），该入口复用与 TCP 完全一致的
Axum 主路由。

### HTTP 路由

Server 在监听端口上同时暴露以下 HTTP 路由：

| 路径                              | 方法        | 说明                                                             |
|---------------------------------|-----------|----------------------------------------------------------------|
| `GET /`                         | GET       | 返回一个包含 Server UUID 和版本信息的 HTML 页面，可用于快速确认服务是否运行                |
| `POST /`                        | POST      | JSON-RPC over HTTP 入口                                          |
| `WS /`                          | WebSocket | JSON-RPC over WebSocket 入口                                     |
| `/worker-route/{route_name}/**` | ANY       | JsWorker HTTP 路由入口，详见 [HTTP Route 绑定](/api/js_worker/route.md) |
| `/terminal`                     | WebSocket | Terminal WebSocket 代理，详见 [Terminal](/api/terminal/index.md)    |
| 其他路径                            | ANY       | **Fallback**: 所有未匹配的路径均转发到 JSON-RPC 服务处理                       |

Fallback 意味着你可以向任意路径发送 JSON-RPC 请求（如 `POST /api`），Server 都会正常处理。

## 数据库

目前兼容了 SQLite 与 PostgreSQL，请根据需要选择。

- 内部测试或小型（Agent 数目 <= 10）可使用 SQLite，性能问题不明显
- 大量 Agent 务必使用 PostgreSQL，表内压缩、`JSONB` 等特性比 SQLite 更省空间，更高效

Server 启动时会自动执行数据库迁移（`Migrator::up()`），无需手动建表或执行 SQL 脚本。版本升级后首次启动即可完成表结构变更。

使用 SQLite 时，Server 会自动开启 WAL（Write-Ahead Logging）模式（`PRAGMA journal_mode=WAL`），以提升并发读写性能。无需手动配置。

## 注意特点

- 任何功能，均不依赖其他功能
  例如：`上报监控信息` 与 `Task 任务获取` 可以在不同地方实现，或只实现其中一个，不影响使用
- UUID 唯一: 虽然可以用户指定每一个 Server / Agent 的 UUID，但设置为 auto_gen 时会在首次启动时随机生成并持久化到配置文件，只要不刻意改变，UUID 也不会改变
  整个系统内只有 UUID 作为唯一辨别 ID，不存在 `name` / `id` 等易混淆字段

---
outline: deep
---

# 架构概述

该项目使用 Rust 作为开发语言，若你需要进行二次开发，请务必熟读该文档

- `Agent` / `探针端` / `客户端` 等均指代 `nodeget-agent`
- `Server` / `服务端` / `主控` 等均指代 `nodeget-server`
- `调用者` / `前端` / `第三方项目` 等均指代使用 `nodeget-server` 提供的 JsonRpc API 进行处理、展示、使用的项目

## 总览

本项目分为三个部分:

- nodeget-agent: 监控 Agent
- nodeget-server: 服务端，提供 API
- nodeget-lib: 用于存放公共结构体、以及一些 utils 代码

目前还是传统 Client / Server 架构

## 基本亮点

目前版本在前人的基础上实现下列功能:

- Agent 多 Server 上报: 一个 Agent 可同时连接多个 Server 上报
- Server 提供的令牌可细粒度权限控制，精确到每一个 Agent、每一个数据项 (即将实现)
- Server 与 Server 互联，交换数据，目标是在 **细粒度权限** 下控制**每一个 Server** 下的**每一个 Agent** (TODO)

## 通信协议

推荐阅读: <https://wiki.geekdream.com/Specification/json-rpc_2.0.html>

`nodeget-server` 提供了一个 WebSocket JsonRpc 服务器，并在同端口同样提供 Http Post JsonRpc 服务器，除无法进行长连接外与
WebSocket JsonRpc 无异

推荐使用 JsonRpc 进行二次开发时同时兼容 WebSocket 与 Http，并优先使用 WebSocket 通信

在非 Windows 平台可选启用 Unix Socket 监听（`enable_unix_socket` / `unix_socket_path`），该入口复用与 TCP 完全一致的
Axum 主路由。

## 数据库

目前兼容了 Sqlite 与 PostgreSQL，请根据需要选择

- 内部测试或小型 (Agent 数目 <= 5) 可使用 Sqlite，性能问题不明显
- 大量 Agents 务必使用 PostgreSQL，表内压缩、JsonBinary 等特性比 Sqlite 更省空间，更高效

## 注意特点

- 任何功能，均不依赖其他功能
  例如：`上报监控信息` 与 `Task 任务获取` 可以在不同地方实现，或只实现其中一个，不影响使用
- UUID 唯一: 虽然可以用户指定每一个 Server / Agent 的 UUID，但会根据环境自动生成，各系统下只要不刻意改变，UUID 也不会改变
  整个系统内只有 UUID 作为唯一辨别 ID，不存在 `name` / `id` 等易混淆字段

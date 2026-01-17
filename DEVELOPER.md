# DEVELOPER.md

本文件用于描述该项目各部分内容的设计

## 总览

本项目分为三个部分:

- nodeget-agent
- nodeget-server
- nodeget-lib: 用于存放公共结构体、以及一些 utils 代码

## nodeget-lib

该 Lib 提供了两个 features: `for-agent` / `for-server`，分别用于 agent / server

项目结构:

- `config`: 配置文件解析模块
    - `agent`: agent 配置文件解析
    - `server`: server 配置文件解析
- `utils`: 工具函数
    - `error_message`: Server Jsonrpc 错误信息结构 / 生成
    - `version`: 在应用内获取编译环境、版本信息
    - `mod`: 杂物，包括 `唯一 UUID 生成`、`时间戳获取`...
- `monitoring`: 监控相关
    - `data_structure`: 监控数据结构，最顶层为 `StaticMonitoringData` / `DynamicMonitoringData`
    - `query`: 为 Server 提供的获取监控数据的字段与条件，用于构造 SQL 查询语句

## nodeget-agent

项目结构:

- `monitoring`: 监控相关
    - `network_connections` / `system_impls` / `gpu` / `impls`: 实现各种数据监控查询
    - 提供顶层 API: `StaticMonitoringData::refresh_and_get()` / `DynamicMonitoringData::refresh_and_get()`，可直接刷新并获取由 `nodeget-lib` 规定的监控数据结构 `StaticMonitoringData` / `DynamicMonitoringData`
- `task`: Server 下发任务
    - `ping`: 实现 ICMP / TCP / HTTP Ping (未使用)
    - `web_shell`: 实现 Web Shell Pty (未实现)
    - ...
- `rpc`: 与 Server 进行 Websocket Jsonrpc 通信
    - `mod`: 定义 Jsonrpc 结构体与简单封装
    - `multi_server`: 维护的多服务器连接池，实现自动重连等，可通过 `send_to()` / `subscribe_to()` 快捷与 任意已连接的 Server 进行通信
    - `monitoring_data_report`: 实现 Static / Dynamic 监控数据定时上报的线程

## nodeget-server

项目结构:

- `migration`: Sea-orm 数据迁移，用于生成数据库结构
- `src`
    - `entity`: 由 `migration` 生成的数据库结构
    - `rpc`: Jsonrpsee 方法实现
        - `nodeget`: 无关紧要的方案 (Hello Ping 与 Version) (后续会加更多功能)
        - `agent`: 与 Agent 通信 / 提供 API 给数据获取者
            - `report`: 接收 Agent 上报的数据
            - `query`: 提供数据查询
    - `db_connection`: 数据库维护
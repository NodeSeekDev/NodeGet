<div align="center">

# **NodeGet**

![logo](https://nodeget.com/logo.png)

***Next-generation server monitoring and management tools***

</div>

> [!IMPORTANT]
> **NOTICE**: NodeGet 正处于 ***早期阶段***，如果你担心安全问题，请在正式发布 **v1.0.0** 版本后再进行使用 *(预计 2-3 周以内)*。遇到问题请及时报告


## **简介**

NodeGet 是一款基于 ***Rust*** 语言编写的服务器管理、监控软件

本 NodeGet 具有超级牛力：

- **完善的细粒度权限支持**：以 Token 系统为基础，所有系统都**强依赖本系统**，实现完全自定义的权限支持
- **极高的可拓展性**：KV 系统实现任意数据存储，Js Worker 实现在原有架构基础上**无限向上拓展功能**。官方提供认证的 Js Worker 方便日常使用
- **现代化的技术栈**：使用 ***Rust*** 作为底层语言，配合 ***PostgreSQL / SQLite*** 储存数据。

  数据通信使用 **WebSocket + JSON-RPC** 主流方案，符合现代微服务架构
- **极度活跃的开发社区**：你想要的冷门功能官方没有实现？

  没问题，官方前端 / 后端均提供了大量插件模块化设计。

  你可以自定义携带的电池重量，完全设计属于你的探针
- **完全前后端分离**：彻底的**前后端分离**，所有操作都走 **JSON-RPC API** 接口。

  允许前端开发者自由构建展示页面和控制面板，并部署到静态储存
- **极高性能**：在塞入如此之多功能之下，各部分占用仍保持在同类项目水准线下，甚至更低
- **极致的网络安全性**：整个项目对外网络请求除了 Agent-Server 必要通信外，只有 NTP 服务器同步与 Ping / IP 等只有用户才可以触发的功能
- **Agent 原生多 Server 支持**：不需要运行多个 Agent，只需要运行一个即可同时与无限多的 Server 通信，并且互不干扰
- **与社区紧密相连**：NodeGet 发根与 Nodeseek 社区，但从未强制与社区绑定起来。你可以自由地使用 NodeGet，并在社区中找到更多玩法
- 别说了，用了才知道

## **相关链接**

- **官方文档**：<https://nodeget.com>
- **Server Docker 部署**：[docker/server/README.md](docker/server/README.md)
- **设计 Intro**：<https://www.nodeseek.com/post-704497-1>
- **Telegram 频道**：[@NodeGetProject](https://t.me/NodeGetProject)
- **Telegram 讨论组**：[@NodegetGroup](https://t.me/NodegetGroup)
- **前端 Dashboard 仓库**：[NodeSeekDev/NodeGet-board](https://github.com/NodeSeekDev/NodeGet-board)
- **前端 Status Board 仓库**：[NodeSeekDev/NodeGet-StatusShow](https://github.com/NodeSeekDev/NodeGet-StatusShow)
- **Nodeseek 社区**：<https://nodeseek.com>

## **误区**

### *NodeGet 是一个怎样的探针系统*

NodeGet 整个项目 ***并不是一个简单的探针项目***。相反，探针只是其微不足道的功能之一

它更类似于 [Komari](https://github.com/komari-monitor/komari) / [Nezha](https://github.com/nezhahq/nezha) 探针系统 + [1Panel](https://1panel.cn/) 管理面板的结合体

你**并不一定需要**从其他类似项目迁移到此，NodeGet 可以很好地与同类项目相结合

### *占用大不大*

可以肯定的是，Agent 的占用情况，不论是从 Binary 体积、运行时峰值 CPU 占用、内存占用来说，都是属于监控项目 **第一梯队** 的级别

Server 端由于塞入了过多功能，可能导致 Binary 体积膨胀 *(仍然可以属于同类较低，相较于本项目 Agent 而言)*

同时运行时由于引入了 JS 语言的 FFI，并且内存缓存机制较为复杂，可能会导致极少部分情况下内存占用激增。

我们正在尽力解决这种情况，非常欢迎提交相关 **Issue / PR**，建议提供详细的复现信息

### *项目有没有 Bugs*

没有一个大型项目是没有 Bugs 的，特别是 NodeGet 是由 ***一个人*** 编写的后端

请务必在遇到任何 Bugs 时候，提交 **Issue** 并详细说明情况

本项目还处于早期阶段，若担心有安全性问题可以待到稳定后再尝试

## **LICENSE**

NodeGet 后端以 **`AGPLv3`** 协议开源，包括 *Server*、*Agent* 与 *Lib*

NodeGet 文档以 **`CC BY 4.0`** 协议开源，范围包括 `docs` 目录下的所有 Markdown 文档

如果无另外说明，以上协议不包括 NodeGet 衍生项目，包括但不限于前端以及第三方代码

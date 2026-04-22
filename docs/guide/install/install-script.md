# 自动化脚本安装

> ⚠️ 注意：自动化脚本安装目前**仅支持 Linux 系统**，Windows 用户需要手动安装或等待社区支持。


## 整体安装流程

完整流程如下：

1. 运行“安装主控”脚本（无特殊需求直接回车）
2. 使用 Nginx / Cloudflare 配置 HTTPS 反向代理，或使用预览 URL
3. 打开控制面板：https://dash.nodeget.com ，添加主控
4. 进入「被控管理」，生成被控安装命令
5. 在被控节点执行命令，等待自动注册完成


## 安装主控

### 默认安装（推荐）

如果没有自建dashboard的打算，执行：
```shell
bash <(curl -sL https://install.nodeget.com) install-server
```
### 自建 Dashboard

如果打算自建dashboard网页，那么需要设定相关的环境变量
```shell
dashboard_url=https://YOUR_DASHBOARD.com \
bash <(curl -sL https://install.nodeget.com) install-server
```

## ⚠️ 重要：HTTPS 与预览模式

因为下面的原因，你必须使用https网关来反向代理 主控的 ws:// 协议监听地址
- 浏览器安全策略（混合内容限制）禁止 HTTPS 页面加载不安全的 ws:// 资源
- dashboard 页面是 https only 的，要连接到主控的接口只能使用 wss 协议
- 为了安全起见，也不应该让superToken（根 token ）在公网直接明文传输

### 增加https网关的办法

- 方法有很多，可以利用nginx反向代理，这是官方推荐的办法
- 也可以 利用 Cloudflare Flexible SSL + Origin Rules，利用 cdn 来提供 https 证书
- 考虑到很多新用户想第一时间尝鲜，而不是在复杂的配置环节花费过多时间，我们增加了预览模式

### 何为预览模式

其原理是利用 cloudflare 的 quick tunnel 服务建立一个临时的内网穿透域名，这样就可以快速体验 NodeGet

虽然 quick tunnel 降低了尝试的门槛，但并不适合作为长期使用方案，服务器重启或者网络波动都有可能导致程序退出

### 如何从预览模式切换到稳定状态

因此如果你在使用预览模式后感觉 NodeGet 适合你，请尽快改为长期的网络地址，具体来说

1. 首先添加新的 https 网关，推荐基于 nginx 或者 cloudflare
2. 然后在面板上的节点设置 => 配置管理 修改当前的主控 URL
3. 如果因为主控无法连接而无法使用面板，推荐用ssh终端连接agent，并用编辑器修改 agent 的[配置](/guide/config/agent.md)
4. 最后修改面板上主控管理记录的 URL 接口地址

## 安装被控
一般来说仅建议在面板上添加被控，因为会自动执行一些安装后的后处理，包括
- 初始化agent namespace的kv，并设定一些元信息
- 添加预设的cron任务，用于延迟探测等等

但如果你有高级的定制需求，可以使用命令行安装，然后手动补充后处理

```shell
bash <(curl -sL https://install.nodeget.com) install-agent
```

## 自动化安装脚本的详细用法

自动化安装脚本支持交互式和非交互式运行，当提供了所有参数后会非交互式运行

当有未确定参数时不会报错，而是（交互式）提醒用户输入


```
Usage:
  install.sh <command> [options]

Commands:
  install-server         安装服务端
  install-agent          安装客户端
  help                   显示帮助

----------------------------------------
install-server 选项:

  --server-ws <addr>     WebSocket 监听地址 (默认: 0.0.0.0:2211)
  --server-id <uuid>     服务端 ID (可选，默认自动生成)
  --db <url>             postgres/sqlite数据库连接字符串 (默认采用sqlite)
  --tunnel <true|false>  是否创建 Cloudflare 临时隧道

----------------------------------------
install-agent 选项:

  --server-ws <url>      服务端 WebSocket 地址 (必填)
  --server-name <name>   节点名称 (默认: 主机名)
  --token <token>        认证 Token (必填)
  --agent-id <uuid>      客户端 ID (可选，默认自动生成)
  --server-id <uuid>     绑定的服务端 ID (可选)

----------------------------------------
示例:

  # 交互模式
  ./install.sh

  # 安装 Server
  ./install.sh install-server \
    --server-ws 0.0.0.0:2211 \
    --db sqlite://nodeget.db?mode=rwc

  # 安装 Agent
  ./install.sh install-agent \
    --server-ws ws://example.com:2211 \
    --token YOUR_TOKEN


交互式界面：

================================
        NodeGet 管理脚本
================================

1. 安装 Server
2. 卸载 Server
3. 更新 Server
4. 查看 Server UUID

5. 安装 Agent
6. 卸载 Agent
7. 更新 Agent

0. 退出

```
# 自动化脚本安装

> ⚠️ 注意：自动化脚本安装目前**仅支持 Linux 系统**。不过 CI 已经构建了 `nodeget-server` 和 `nodeget-agent` 的 Windows
> x86_64 / aarch64 版本，Windows 用户可以直接从 GitHub Releases 下载 `.exe` 二进制文件。

## 整体安装流程

完整流程如下：

1. 运行"安装主控"脚本（无特殊需求直接回车）
2. 使用 Nginx / Cloudflare 配置 HTTPS 反向代理，或使用预览 URL
3. 打开控制面板：https://dash.nodeget.com ，添加主控
4. 进入「被控管理」，生成被控安装命令
5. 在被控节点执行命令，等待自动注册完成

## 安装主控

### 默认安装（推荐）

如果没有自建 Dashboard 的打算，执行：

```shell
bash <(curl -sL https://install.nodeget.com) install-server
```

### 自建 Dashboard

如果打算自建 Dashboard 网页，那么需要设定相关的环境变量：

```shell
dashboard_url=https://YOUR_DASHBOARD.com \
bash <(curl -sL https://install.nodeget.com) install-server
```

## ⚠️ 重要：HTTPS 与预览模式

因为下面的原因，你必须使用 HTTPS 网关来反向代理主控的 `ws://` 协议监听地址：

- 浏览器安全策略（混合内容限制）禁止 HTTPS 页面加载不安全的 `ws://` 资源
- Dashboard 页面是 HTTPS only 的，要连接到主控的接口只能使用 `wss` 协议
- 为了安全起见，也不应该让 SuperToken（根 Token）在公网直接明文传输

### 增加 HTTPS 网关的办法

- 方法有很多，可以利用 Nginx 反向代理，这是官方推荐的办法
- 也可以利用 Cloudflare Flexible SSL + Origin Rules，利用 CDN 来提供 HTTPS 证书
- 考虑到很多新用户想第一时间尝鲜，而不是在复杂的配置环节花费过多时间，我们增加了预览模式

### 何为预览模式

其原理是利用 Cloudflare 的 Quick Tunnel 服务建立一个临时的内网穿透域名，这样就可以快速体验 NodeGet。

虽然 Quick Tunnel 降低了尝试的门槛，但并不适合作为长期使用方案，服务器重启或者网络波动都有可能导致程序退出。

### 如何从预览模式切换到稳定状态

因此如果你在使用预览模式后感觉 NodeGet 适合你，请尽快改为长期的网络地址，具体来说：

1. 首先添加新的 HTTPS 网关，推荐基于 Nginx 或者 Cloudflare
2. 然后在面板上的节点设置 => 配置管理修改当前的主控 URL
3. 如果因为主控无法连接而无法使用面板，推荐用 SSH 终端连接 Agent，并用编辑器修改 Agent 的 [配置](/guide/config/agent.md)
4. 最后修改面板上主控管理记录的 URL 接口地址

## 安装被控

一般来说仅建议在面板上添加被控，因为会自动执行一些安装后的后处理，包括：

- 初始化 Agent namespace 的 Kv，并设定一些元信息
- 添加预设的 Cron 任务，用于延迟探测等等

但如果你有高级的定制需求，可以使用命令行安装，然后手动补充后处理：

```shell
bash <(curl -sL https://install.nodeget.com) install-agent
```

## 安装预发布版和 nightly

NodeGet 有三档发布，不指定版本时脚本只会安装**正式版**：

| 档位 | 版本号示例 | 说明 |
|---|---|---|
| 正式版 | `v0.6.1` | 默认，普通用户只使用这一档 |
| 预发布 | `v0.6.1-beta.1`、`v0.6.1-rc.1` | 正式发布前给愿意试用的用户，beta 功能可能还在变化，rc 已冻结功能、只修阻塞问题 |
| nightly | `nightly` | 每天从开发分支自动构建，滚动更新，包含最新改动 |

预发布版和 nightly **不保证稳定**，请不要用于生产环境。

用环境变量 `releases_tag` 指定要安装的版本，`install-*` 和 `update-*` 命令都支持：

```shell
# 安装 / 升级到 nightly
releases_tag=nightly bash <(curl -sL https://install.nodeget.com) install-server
releases_tag=nightly bash <(curl -sL https://install.nodeget.com) update-agent

# 安装 / 升级到某个预发布版本
releases_tag=v0.6.1-rc.1 bash <(curl -sL https://install.nodeget.com) update-server
```

想回到正式版，不带 `releases_tag` 重新执行 `update-server` / `update-agent` 即可。

nightly 每次构建的版本号不变，所以每次执行 `update-*` 都会重新下载并重启。Docker 用户请见 [Docker 安装](./docker.md)。

## 自动化安装脚本的详细用法

自动化安装脚本支持交互式和非交互式运行，当提供了所有参数后会非交互式运行。

当有未确定参数时不会报错，而是（交互式）提醒用户输入。

```
Usage:
  install.sh <command> [options]

Commands:
  install-server         安装服务端
  install-agent          安装客户端
  update-server          升级服务端
  update-agent           升级客户端
  uninstall-server       卸载服务端
  uninstall-agent        卸载客户端
  help                   显示帮助

----------------------------------------
install-server 选项:

  --server-ws <addr>     WebSocket 监听地址 (默认: 0.0.0.0:2211)
  --server-id <uuid>     服务端 ID (可选，默认首次启动时随机生成并持久化)
  --db <url>             PostgreSQL/SQLite 数据库连接字符串 (默认采用 SQLite)
  --tunnel <true|false>  是否创建 Cloudflare 临时隧道

----------------------------------------
install-agent 选项:

  --server-ws <url>      服务端 WebSocket 地址 (必填)
  --server-name <name>   节点名称 (默认: 主机名)
  --token <token>        认证 Token (必填)
  --agent-id <uuid>      客户端 ID (可选，默认首次启动时随机生成并持久化)
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
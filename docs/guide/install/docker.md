# Docker 安装

## 安装 Server

官方 Docker 镜像由 CI 从源码编译并推送到 Docker Hub：

- `genshinmc/nodeget:latest`：最新发布版本镜像
- `genshinmc/nodeget:vX.Y.Z`：Release tag 镜像

### PostgreSQL + NodeGet

先进入一个用于存放 NodeGet Server 配置和数据的目录，然后执行：

```shell
curl -fsSL https://raw.githubusercontent.com/NodeSeekDev/NodeGet/main/docker-compose.postgres.yml -o docker-compose.yml
docker compose up -d
```

### SQLite + NodeGet

同样先进入一个用于存放 NodeGet Server 配置和数据的目录，然后执行：

```shell
curl -fsSL https://raw.githubusercontent.com/NodeSeekDev/NodeGet/main/docker-compose.sqlite.yml -o docker-compose.yml
docker compose up -d
```

数据会保存在当前目录的 `./data` 下：

```text
data/
  config/
    config.toml
  sqlite/
    nodeget.db
  postgres/
```

`./data/config/config.toml` 是 NodeGet 配置文件。SQLite 部署使用 `./data/sqlite`，PostgreSQL 部署使用 `./data/postgres`。删除容器不会删除这些目录；如需清空数据，请停止服务后手动删除对应目录。

默认暴露 `2211` 端口。

服务**首次启动**时会生成 Super Token。可在 `docker-compose.yml` 所在目录执行下面的命令查看：

```shell
docker compose logs nodeget | grep -E 'Super Token'
```

获取 Super Token 后，可前往 [NodeGet Dashboard 的 Server 管理页面](https://dash.nodeget.com/#/dashboard/node-manage?tab=servers) 添加 Server。

如需修改镜像 tag、端口映射、数据库账号等 Docker 部署参数，请编辑下载下来的 `docker-compose.yml`。

`./data/config/config.toml` 生成后，NodeGet 的运行配置以这个文件为准。修改监听地址、日志级别、数据库地址等 NodeGet 配置时，请编辑 `./data/config/config.toml`；只改 `docker-compose.yml` 不会覆盖已有配置文件。

## 安装 Agent

待支持

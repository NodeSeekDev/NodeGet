# Docker 安装

## 安装 Server

官方 Docker 镜像由 CI 从源码编译并推送到 Docker Hub：

- `genshinmc/nodeget:latest`：最新发布版本镜像
- `genshinmc/nodeget:vX.Y.Z`：Release tag 镜像

### PostgreSQL + NodeGet

```shell
curl -fsSL https://raw.githubusercontent.com/NodeSeekDev/NodeGet/main/docker-compose.postgres.yml -o docker-compose.yml
docker compose up -d
```

### SQLite

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

默认暴露 `3000` 端口。

如需修改镜像 tag、端口映射、数据库账号等 Docker 部署参数，请编辑下载下来的 `docker-compose.yml`。

`./data/config/config.toml` 生成后，NodeGet 的运行配置以这个文件为准。修改监听地址、日志级别、数据库地址等 NodeGet 配置时，请编辑 `./data/config/config.toml`；只改 `docker-compose.yml` 不会覆盖已有配置文件。

## 安装 Agent

待支持

# Docker 安装

官方 Docker 镜像由 CI 从源码编译并推送到 Docker Hub：

- `genshinmc/nodeget:latest`：最新发布版本镜像
- `genshinmc/nodeget:vX.Y.Z`：Release tag 镜像

## 一键

```bash
docker run -d \
  --name nodeget \
  -p 2211:2211 \
  -v /your/host/path:/nodeget \
  genshinmc/nodeget:latest
```

该命令默认使用 `sqlite:///nodeget/nodeget.db?mode=rwc` 为数据库

自动生成的配置文件如下: 

```toml
ws_listener = "0.0.0.0:2211"
server_uuid = "auto_gen"

[logging]
log_filter = "info"

[database]
database_url = "sqlite:///nodeget/nodeget.db?mode=rwc"
```

同时也可传入 `NODEGET_DATABASE_URL` 环境变量覆盖默认数据库配置：

```bash
docker run -d \
  --name nodeget \
  -p 2211:2211 \
  -v /your/host/path:/nodeget \
  -e NODEGET_DATABASE_URL="postgresql://user:password@host:port/dbname" \
  genshinmc/nodeget:latest
```

## Docker Compose

### SQLite

```bash
curl -o docker-compose.yml https://raw.githubusercontent.com/NodeSeekDev/NodeGet/refs/heads/main/docker/docker-compose.sqlite.yml
docker compose up -d
```

### PostgreSQL

```bash
curl -o docker-compose.yml https://raw.githubusercontent.com/NodeSeekDev/NodeGet/refs/heads/main/docker/docker-compose.postgres.yml
docker compose up -d
```

> 生产环境请务必修改 `POSTGRES_PASSWORD` 与 `NODEGET_DATABASE_URL` 中的密码。

## 获取 Super Token

```shell
docker logs nodeget | grep -E 'Super Token'
```

获取 Super Token 后，可前往 [NodeGet Dashboard 的 Server 管理页面](https://dash.nodeget.com/#/dashboard/node-manage?tab=servers) 添加
Server。
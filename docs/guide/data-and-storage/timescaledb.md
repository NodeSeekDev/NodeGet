# TimescaleDB 部署指南

NodeGet 的数据主体是每秒一条的监控时序数据（`dynamic_monitoring`、`dynamic_monitoring_summary`）。
当数据库是安装了 **timescaledb** 扩展的 PostgreSQL 时，NodeGet 会在**启动时自动**完成时序优化，无需改业务代码：

1. 把上述时序表转换为 **hypertable**（含存量数据自动迁移 `migrate_data`；首次转换会随存量数据量拉长启动时间，属一次性成本）；
2. 注册整数时间 now() 函数（`set_integer_now_func`）；
3. 对 `compress_after_days` 天前的 chunk 启用 **zstd 列式压缩**（监控 JSON 数据通常可压缩 90% 以上）；
4. 当配置 `retention_days > 0` 时，注册**自动保留策略**（`drop_chunks`），超过该天数的数据由后台 job 自动删除，磁盘占用从此有上界。

> [!NOTE]
> 仅当连接的是安装 timescaledb 扩展的 PostgreSQL 时生效；普通 PostgreSQL / SQLite 部署完全不受影响。
> 未安装扩展时启动日志会显示 `timescaledb extension not installed; skipping timescale setup`，直接跳过。

> [!WARNING]
> **hypertable 转换不可逆**：转换后表结构（主键从 `(id)` 变为 `(id, timestamp)`）依赖 timescaledb 扩展，
> 普通 PostgreSQL（无扩展）无法读取。降级/回退需要先用 TimescaleDB 官方迁移工具 `untable`
> 转换回普通表，或提前做好备份。
> 另外，**主键契约发生变化**：`id` 不再是唯一约束列（仍为自增且全局唯一）。现有业务代码不受影响，
> 但转换后 `REFERENCES <table>(id)` 外键或 `ON CONFLICT (id)` 语句将不再合法，需改用 `(id, timestamp)`。

## 前置条件

- PostgreSQL 17（推荐）或 16，安装 `timescaledb` 扩展。Docker 直接用官方镜像 `timescale/timescaledb:2.29.1-pg17`（固定版本，便于复现与升级管理）。
- NodeGet Server 二进制需包含本功能（见下方构建）。

## 一、构建含 TimescaleDB 支持的镜像

官方预构建镜像（`genshinmc/nodeget`）不含本功能，需要**用自己的代码构建**。推荐直接用仓库自带的 GitHub Actions release workflow：

1. 将代码推送到你的 fork：
   ```bash
   git push origin timescale-backend
   ```
2. 在 fork 的 **Settings → Secrets and variables → Actions** 配置 Docker Hub 凭据：
   - `DOCKERHUB_USERNAME` — Docker Hub 用户名
   - `DOCKERHUB_TOKEN` — Docker Hub Access Token（需 Read & Write 权限）
3. 把 release workflow 中 `IMAGE_NAME` 改为你自己的 Docker Hub 镜像名（如 `yourname/nodeget`）。
4. 在 **Actions** 页手动运行 `release` workflow，勾选 **publish_release**。
5. 完成后 Docker Hub 会出现 `yourname/nodeget:latest`（多架构 amd64/arm64），GitHub Release 附带各平台二进制。

> 也可以在本机构建：`cargo build --release`（需要 Rust 工具链），或 `cross build --package nodeget-server --target <target> --profile minimal` 交叉编译。

## 二、Docker Compose 部署

```yaml
name: nodeget-timescale

services:
  timescaledb:
    image: timescale/timescaledb:2.29.1-pg17
    restart: unless-stopped
    environment:
      POSTGRES_DB: nodeget
      POSTGRES_USER: nodeget
      POSTGRES_PASSWORD: 换成强密码
    volumes:
      - ./data/timescaledb:/var/lib/postgresql/data
    healthcheck:
      test: [ "CMD-SHELL", "pg_isready -U nodeget -d nodeget" ]
      interval: 5s
      timeout: 5s
      retries: 20

  nodeget:
    image: yourname/nodeget:latest
    restart: unless-stopped
    depends_on:
      timescaledb:
        condition: service_healthy
    environment:
      NODEGET_DATABASE_URL: postgres://nodeget:换成强密码@timescaledb:5432/nodeget
      # 显式启用自动删除：超过 30 天的数据自动清理（磁盘封顶）
      NODEGET_TIMESCALE_RETENTION_DAYS: "30"
    ports:
      - "2211:2211"
    volumes:
      - ./data/nodeget:/nodeget
```

> 仓库 `docker/docker-compose.timescale.yml` 提供了同款模板（将镜像名替换为你的构建产物即可）。

启动：

```bash
docker compose up -d
docker compose logs -f nodeget
```

看到以下日志即表示时序优化生效：

```
timescaledb detected; applying hypertable setup
converted to hypertable          table=dynamic_monitoring
compression enabled              table=dynamic_monitoring
compression policy applied
retention policy applied
```

> 中国大陆服务器拉取 Docker Hub 镜像超时时，可使用镜像加速器，例如：
> ```bash
> docker pull docker.m.daocloud.io/timescale/timescaledb:2.29.1-pg17
> docker tag docker.m.daocloud.io/timescale/timescaledb:2.29.1-pg17 timescale/timescaledb:2.29.1-pg17
> ```
> 你自己的镜像同样处理（把 `timescale/timescaledb` 换成 `yourname/nodeget`）。

## 三、配置参数

`[database.timescale]` 段，均可省略（使用默认值）：

| 参数 | 默认 | 说明 |
|---|---|---|
| `chunk_interval_days` | `1` | hypertable 分块间隔（天），影响 chunk 数量与压缩/删除粒度 |
| `compress_after_days` | `7` | 距离当前时间超过该天数的 chunk 启用 zstd 列式压缩；**`0` = 所有数据立即可压缩**（含实时数据，压缩 job 会频繁执行，一般不建议） |
| `retention_days` | `0` | 超过该天数的数据自动删除；**默认 0 = 不启用**（避免误删历史数据），需显式配置 |

> [!NOTE]
> 若 `retention_days < compress_after_days`，数据会在压缩策略生效**之前**就被保留策略删除
> （删除优先于压缩）。如希望"先压缩保留、到期再删"，请保持 `retention_days > compress_after_days`。

Docker 部署时通过 entrypoint 生成配置（镜像内的 `docker/entrypoint.sh`），环境变量：

```bash
NODEGET_TIMESCALE_CHUNK_INTERVAL_DAYS
NODEGET_TIMESCALE_COMPRESS_AFTER_DAYS
NODEGET_TIMESCALE_RETENTION_DAYS
```

若已挂载自定义 `config.toml`，直接在文件中写：

```toml
[database]
database_url = "postgres://nodeget:password@timescaledb:5432/nodeget"

[database.timescale]
chunk_interval_days = 1
compress_after_days = 7
retention_days = 30
```

## 四、数据迁移（从普通 PostgreSQL）

1. 停服旧 NodeGet；
2. 导出：`pg_dump -U nodeget -d nodeget -Fc -f nodeget.dump`（或用 `docker compose exec postgres pg_dump ...`）；
3. 导入到 timescaledb 容器：`pg_restore -U nodeget -d nodeget nodeget.dump`（注意先建好空库）；
4. 启动新 NodeGet，首次启动自动把存量数据迁入 hypertable。

> [!WARNING]
> `retention_days` 注册的删除策略会**立即删除**超过阈值的存量 chunk。
> 迁移历史数据时若不确定是否需要保留，请先保持 `retention_days = 0`（默认），
> 清理完旧数据后再显式开启；或先自行清理旧数据再迁移。

## 五、验证

```bash
# 进入 timescaledb 容器
docker exec -it nodeget-timescale-timescaledb-1 psql -U nodeget -d nodeget
```

```sql
-- hypertable 与压缩是否启用
SELECT hypertable_name, compression_enabled FROM timescaledb_information.hypertables;

-- 压缩/保留策略
SELECT job_id, proc_name, config
FROM timescaledb_information.jobs
WHERE hypertable_name IN ('dynamic_monitoring', 'dynamic_monitoring_summary');

-- 压缩效果（过 compress_after_days 后手动跑一次 job 或等待后台调度）
CALL run_job(<policy_compression 的 job_id>);
SELECT count(*) FILTER (WHERE is_compressed) AS compressed_chunks
FROM timescaledb_information.chunks
WHERE hypertable_name = 'dynamic_monitoring';
```

策略参数（`compress_after_days` / `retention_days`）修改后**重启 NodeGet 即生效**（每次启动按配置重新注册策略）。

## 六、从零开始（无历史数据）

新库启动即完成建表 + hypertable 转换（空表迁移无成本）。之后：

1. 用新 server 的 super token 登录管理端；
2. 创建新的 agent token；
3. 更新每台 agent 的配置（server 地址 + 新 token）并重启 agent。

## 常见问题

- **Q：普通 PostgreSQL 上会有什么影响？** 无影响。未安装 timescaledb 扩展时整个模块跳过。
- **Q：hypertable 转换可逆吗？** 不可逆。转换后表依赖 timescaledb 扩展，普通 PostgreSQL 无法读取；回退需用 TimescaleDB 官方 `untable` 工具或提前备份。启用前请确认规划。
- **Q：转换会影响现有代码吗？** 现有按 `id` 增删改查的代码不受影响（`id` 仍全局唯一）；但表主键实际变为 `(id, timestamp)`，此后新增 `REFERENCES <table>(id)` 外键或 `ON CONFLICT (id)` 语句不再合法，需要包含 `timestamp` 列。
- **Q：首次启动为什么变慢？** `migrate_data => true` 会把存量数据迁入 hypertable，数据量越大耗时越长（一次性成本，后续启动不重复迁移）。
- **Q：`set_integer_now_func` 报错？** 已内置幂等检查（已设置则跳过），正常不会报错。
- **Q：为什么默认不启用 `retention_days`？** 自动删除会立即作用于存量数据，默认关闭以保护历史数据，需要时显式开启。
- **Q：压缩为什么没立即生效？** 压缩策略只处理 `compress_after_days` 之前的 chunk（默认 7 天），新数据需要时间自然过期；也可手动 `CALL run_job(<job_id>)` 提前压缩。
- **Q：初始化部分失败会怎样？** 单表转换/策略失败不阻断服务启动（以普通表继续运行），但启动日志会输出醒目的失败横幅，请按提示检查修复后重启；只有扩展检测或 now() 函数创建失败才会阻断启动。

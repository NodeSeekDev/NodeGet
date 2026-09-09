# TimescaleDB 部署指南

NodeGet 的数据主体是每秒一条的监控时序数据（`dynamic_monitoring`、`dynamic_monitoring_summary`）。
当数据库是安装了 **timescaledb** 扩展的 PostgreSQL 时，NodeGet 会在**启动时自动**完成时序优化，无需改业务代码：

1. 把上述时序表转换为 **hypertable**（含存量数据自动迁移 `migrate_data`；首次转换会随存量数据量拉长启动时间，属一次性成本）；
2. 注册整数时间 now() 函数（`set_integer_now_func`）；
3. 对 `compress_after_days` 天前的 chunk 启用 **zstd 列式压缩**（监控 JSON 数据实测可压缩 380~1000 倍）；
4. 当配置 `retention_days > 0` 时，注册**自动保留策略**（`drop_chunks`），超过该天数的数据由后台 job 自动删除，磁盘占用从此有上界（实测：约 1.4 GB/天裸写 → 压缩后 ~2-4 MB/天 → 30 天封顶）。

> [!NOTE]
> 仅当连接的是安装 timescaledb 扩展的 PostgreSQL 时生效；普通 PostgreSQL / SQLite 部署完全不受影响。
> 未安装扩展时启动日志会显示 `timescaledb extension not installed; skipping timescale setup`，直接跳过。

> [!WARNING]
> **hypertable 转换不可逆**：转换后表结构（主键从 `(id)` 变为 `(id, timestamp)`）依赖 timescaledb 扩展，
> 普通 PostgreSQL（无扩展）无法读取。降级/回退需要先用 TimescaleDB 官方迁移工具 `untable`
> 转换回普通表，或提前做好备份。
> 另外，**主键契约发生变化**：`id` 不再是唯一约束列（仍为自增且全局唯一）。现有业务代码不受影响，
> 但转换后 `REFERENCES <table>(id)` 外键或 `ON CONFLICT (id)` 语句将不再合法，需改用 `(id, timestamp)`。
> **扩展版本必须与库一致**：由某版本 timescaledb 初始化的库（`pg_extension` 记录版本）只能由
> **同版本或更高版本**的扩展加载。用低版本镜像接管高版本初始化的库会报
> `could not access file "timescaledb-<ver>"`，请保持镜像 timescaledb 版本 >= 建库时的版本。

## 前置条件

- PostgreSQL 18（推荐，PG16/17 亦可）并安装 `timescaledb` 扩展。
- NodeGet Server 镜像/二进制需含本功能（viogus fork 默认包含）。

## 一、数据库镜像选型

压缩与 retention 策略属于 **Timescale License（TSL）** 部分：Timescale 官方只发布
glibc/Debian 的完整镜像；Alpine 社区包（`postgresql-timescaledb`）用 `-DAPACHE_ONLY=ON`
编译，**不含压缩/retention，不满足 NodeGet 需求**。可用选择：

| 镜像 | 体积 | 内容 | 适用 |
|---|---|---|---|
| `timescale/timescaledb:latest-pg18` | ~1.0 GB | 官方完整版（TSL + toolkit） | 默认/省心 |
| `ghcr.io/viogus/timescaledb:latest-pg18`（tag `2.30.0-pg18`） | ~314 MB | **自编译完整版**（TSL 含压缩/retention，无 toolkit；基于 postgres:18-alpine） | 单机、想省体积 |

`ghcr.io/viogus/timescaledb` 由 viogus/scripts 的 `docker/timescaledb/Dockerfile` 构建
（upstream 默认编译，2.30.0 与官方 latest 对齐，可用 2.30.0 镜像接管官方镜像建的库）。
构建定义：`https://github.com/viogus/scripts/tree/main/docker/timescaledb`。

## 二、Docker Compose 部署

> 也见仓库 `docker/docker-compose.timescale.yml` 与 HostKit `scripts/nodeget-timescale.docker-compose.yml` 模板。

以本仓库（viogus fork）发布链为例 —— NodeGet 镜像 `ghcr.io/viogus/nodeget-server:latest`，
数据库镜像二选一：

```yaml
services:
  timescaledb:
    # 官方完整版（~1 GB）：
    image: timescale/timescaledb:latest-pg18
    # 或自编译小体积版（~314 MB，功能一致）：
    # image: ghcr.io/viogus/timescaledb:latest-pg18
    container_name: nodeget-db
    restart: always
    network_mode: "host"                    # host 网络：直接占用宿主 5432
    command: ["postgres", "-c", "listen_addresses=127.0.0.1", "-c", "shared_preload_libraries=timescaledb"]
    environment:
      POSTGRES_USER: nodeget_user
      POSTGRES_PASSWORD: 换成强密码
      POSTGRES_DB: nodeget_db
    volumes:
      # PG 18+：数据在 <major>/ 子目录，挂载点必须是 /var/lib/postgresql 父目录
      # （docker-library/postgres#1259）；PG 17 及更早挂 /var/lib/postgresql/data
      - /root/nodeget/pgdata:/var/lib/postgresql
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U nodeget_user -d nodeget_db"]
      interval: 5s
      timeout: 5s
      retries: 5

  nodeget:
    image: ghcr.io/viogus/nodeget-server:latest
    container_name: nodeget
    restart: always
    network_mode: "host"
    environment:
      NODEGET_SERVER_UUID: "换成你的固定 uuid"   # 必须固定，否则容器重建 server_uuid 变化导致全部 agent 掉线
      NODEGET_PORT: "2211"
      # 双写：官方 entrypoint.sh 只认 NODEGET_DATABASE_URL；ghcr entrypoint.c 两者都认
      NODEGET_DATABASE_URL: postgresql://nodeget_user:换成强密码@127.0.0.1:5432/nodeget_db
      DATABASE_URL: postgresql://nodeget_user:换成强密码@127.0.0.1:5432/nodeget_db
      # TimescaleDB：30 天自动删除（磁盘封顶）
      NODEGET_TIMESCALE_RETENTION_DAYS: "30"
    volumes:
      # 持久化 config.toml（含 server_uuid/token）：挂 /etc/nodeget 目录，容器重建不丢
      - /root/nodeget/etc:/etc/nodeget
```

启动与验证：

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

> 中国大陆服务器拉取 Docker Hub/ghcr 超时时可用镜像加速，或给 docker 配置 registry mirror。

## 三、直接服务器安装（无 Docker）

在已装 PostgreSQL 18 的服务器上安装 timescaledb 扩展，再直接跑 NodeGet Server 二进制。

1. **安装扩展**（任选一种，见 [TimescaleDB 官方安装文档](https://docs.timescale.com/self-hosted/latest/install/)）：
   - Debian/Ubuntu（Timescale 官方 apt 源，含 TSL 完整功能）：
     ```bash
     apt-get install -y gnupg postgresql-common apt-transport-https lsb-release wget
     /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y
     echo "deb https://packagecloud.io/timescale/timescaledb/ubuntu/ $(lsb_release -c -s) main" > /etc/apt/sources.list.d/timescaledb.list
     wget --quiet -O - https://packagecloud.io/timescaledb/timescaledb/gpgkey | apt-key add -
     apt-get update && apt-get install -y timescaledb-2-postgresql-18
     timescaledb-tune --conf-path /etc/postgresql/18/main/postgresql.conf   # 自动写入 shared_preload_libraries
     systemctl restart postgresql
     ```
   - RHEL/CentOS/Fedora：Timescale 官方 yum/dnf 源安装 `timescaledb-2-postgresql-18`，同样 `timescaledb-tune` + 重启。
   - 自编译（如需 Alpine/musl 或最小构建）：参考本仓库 viogus/scripts `docker/timescaledb/Dockerfile`
     （upstream 默认编译含 TSL；用 `-DAPACHE_ONLY=ON` 会失去压缩/retention，不要用于 NodeGet）。

2. **验证扩展**：
   ```bash
   sudo -u postgres psql
   ```
   ```sql
   CREATE EXTENSION IF NOT EXISTS timescaledb;
   \dx timescaledb
   ```

3. **配置 NodeGet Server**：`config.toml` 指向该库即可，NodeGet 启动时自动完成 hypertable/压缩/retention：
   ```toml
   [database]
   database_url = "postgresql://nodeget_user:换成强密码@127.0.0.1:5432/nodeget_db"

   [database.timescale]
   chunk_interval_days = 1
   compress_after_days = 7
   retention_days = 30
   ```

## 四、配置参数

`[database.timescale]` 段，均可省略（使用默认值）：

| 参数 | 默认 | 说明 |
|---|---|---|
| `chunk_interval_days` | `1` | hypertable 分块间隔（天），影响 chunk 数量与压缩/删除粒度 |
| `compress_after_days` | `7` | 距离当前时间超过该天数的 chunk 启用 zstd 列式压缩；**`0` = 所有数据立即可压缩**（压缩 job 会频繁执行，一般不建议） |
| `retention_days` | `0` | 超过该天数的数据自动删除；**默认 0 = 不启用**（避免误删历史数据），需显式配置 |

> [!NOTE]
> 若 `retention_days < compress_after_days`，数据会在压缩策略生效**之前**就被保留策略删除
> （删除优先于压缩）。如希望"先压缩保留、到期再删"，请保持 `retention_days > compress_after_days`。

Docker 部署时通过 entrypoint 生成配置，环境变量：

```bash
NODEGET_TIMESCALE_CHUNK_INTERVAL_DAYS
NODEGET_TIMESCALE_COMPRESS_AFTER_DAYS
NODEGET_TIMESCALE_RETENTION_DAYS
```

## 五、数据迁移（从普通 PostgreSQL）

1. 停服旧 NodeGet；
2. 导出：`pg_dump -U nodeget -d nodeget_db -Fc -f nodeget.dump`（或用 `docker compose exec postgres pg_dump ...`）；
3. 导入到 timescaledb 容器：`pg_restore -U nodeget -d nodeget_db nodeget.dump`（注意先建好空库）；
4. 启动新 NodeGet，首次启动自动把存量数据迁入 hypertable。

> [!WARNING]
> `retention_days` 注册的删除策略会**立即删除**超过阈值的存量 chunk。
> 迁移历史数据时若不确定是否需要保留，请先保持 `retention_days = 0`（默认），
> 清理完旧数据后再显式开启；或先自行清理旧数据再迁移。

## 六、验证

```bash
docker exec -it nodeget-db psql -U nodeget_user -d nodeget_db
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

-- 磁盘占用是否随时间有上界（重点看 retention 是否在删）
SELECT pg_size_pretty(pg_total_relation_size('dynamic_monitoring'));
```

策略参数（`compress_after_days` / `retention_days`）修改后**重启 NodeGet 即生效**（每次启动按配置重新注册策略）。

## 七、从零开始（无历史数据）

新库启动即完成建表 + hypertable 转换（空表迁移无成本）。之后：

1. 用新 server 的 super token 登录管理端；
2. 创建新的 agent token；
3. 更新每台 agent 的配置（server 地址 + 新 token）并重启 agent。

## 常见问题

- **Q：普通 PostgreSQL 上会有什么影响？** 无影响。未安装 timescaledb 扩展时整个模块跳过。
- **Q：hypertable 转换可逆吗？** 不可逆。转换后表依赖 timescaledb 扩展，普通 PostgreSQL 无法读取；回退需用 TimescaleDB 官方 `untable` 工具或提前备份。启用前请确认规划。
- **Q：换镜像后报 `could not access file "timescaledb-<ver>"`？** 数据库扩展版本高于新镜像提供的版本。请使用 >= 建库时版本的 timescaledb（官方 latest-pg18 与 `ghcr.io/viogus/timescaledb:2.30.0-pg18` 均为 2.30.0，可互相接管）。
- **Q：转换会影响现有代码吗？** 现有按 `id` 增删改查的代码不受影响（`id` 仍全局唯一）；但表主键实际变为 `(id, timestamp)`，此后新增 `REFERENCES <table>(id)` 外键或 `ON CONFLICT (id)` 语句不再合法，需要包含 `timestamp` 列。
- **Q：首次启动为什么变慢？** `migrate_data => true` 会把存量数据迁入 hypertable，数据量越大耗时越长（一次性成本，后续启动不重复迁移）。
- **Q：`set_integer_now_func` 报错？** 已内置幂等检查（已设置则跳过），正常不会报错。
- **Q：为什么默认不启用 `retention_days`？** 自动删除会立即作用于存量数据，默认关闭以保护历史数据，需要时显式开启。
- **Q：压缩为什么没立即生效？** 压缩策略只处理 `compress_after_days` 之前的 chunk（默认 7 天），新数据需要时间自然过期；也可手动 `CALL run_job(<job_id>)` 提前压缩。integer 时间维度的策略参数必须传**整数毫秒**（如 `604800000` 表示 7 天），不能传 `INTERVAL`。
- **Q：初始化部分失败会怎样？** 单表转换/策略失败不阻断服务启动（以普通表继续运行），但启动日志会输出醒目的失败横幅，请按提示检查修复后重启；只有扩展检测或 now() 函数创建失败才会阻断启动。

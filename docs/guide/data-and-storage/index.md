# 数据与储存

在本节总结与 NodeGet 数据储存有关的功能和机制，包括监控数据、Kv 储存、数据占用分析功能等。

## 储存引擎

NodeGet 目前支持 SQLite 和 PostgreSQL 作为储存引擎，对于小规模（30 个以下 Agent）的服务器数量推荐使用前者，对于大规模（大于
200）服务器数量推荐使用后者。

### TimescaleDB（时序优化）

NodeGet 的数据主体是每秒一条的监控时序数据。当数据库是安装了 **timescaledb** 扩展的 PostgreSQL 时，NodeGet 会在启动时自动把
`dynamic_monitoring`、`dynamic_monitoring_summary` 转换为 **hypertable**，并按配置启用列式压缩与自动保留策略：

- **列式压缩**：对 7 天前的 chunk 启用压缩（zstd），监控 JSON 数据通常可压缩 10 倍以上；
- **自动保留**（可选）：配置 `retention_days > 0` 后，超过该天数的数据由后台 job 自动删除（drop_chunks），磁盘占用从此有上界。
  默认 `retention_days = 0` **不启用删除**，避免误删历史数据。

部署方式（Docker）：

```bash
docker compose -f docker/docker-compose.timescale.yml up -d
```

可调参数（均有默认值，非必填）：

```toml
[database]
database_url = "postgres://nodeget:nodeget@host:5432/nodeget"

[database.timescale]
chunk_interval_days = 1    # hypertable 分块间隔（天），默认 1
compress_after_days = 7    # 超过该天数的 chunk 启用压缩，默认 7
retention_days = 30        # 超过该天数的数据自动删除；默认 0 = 不启用
```

> [!WARNING]
> `retention_days` 只对**新注册的策略之后的数据**生效，且默认不启用。
> 从普通 PostgreSQL 迁移存量数据时，若想控制历史数据，建议先自行清理旧数据，
> 再显式配置 `retention_days`；否则迁移进来的超过该天数的历史数据会在策略生效后被删除。

> [!NOTE]
> 仅当连接的是安装 timescaledb 扩展的 PostgreSQL 时生效；普通 PostgreSQL / SQLite 部署完全不受影响。
> 从普通 PostgreSQL 迁移：停服后 `pg_dump` 导出，恢复到 TimescaleDB 容器即可，存量数据会在首次启动时
> 由 hypertable 转换自动迁移。

完整部署指南（构建镜像、Docker Compose、配置、迁移、验证）见 [TimescaleDB 部署指南](./timescaledb.md)。

为了支持用户储存自定义的数据到 NodeGet，通过 SQL 储存模拟实现了一个 Kv 数据库。

这在很多场景都会用到，比如 Js Worker 储存，扩展应用的静态文件储存等等。

## 主控上统计分析

在节点管理功能里打开某个主控的详情页面后，利用储存占用分析功能可以看到各个 SQL 表的储存占用情况。

*会在近期更新控制面板来支持最新的储存占用分析接口*
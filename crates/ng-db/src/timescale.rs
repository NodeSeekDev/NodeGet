//! `TimescaleDB` 时序优化初始化。
//!
//! `NodeGet` 的数据主体是每秒一条的监控时序数据（`dynamic_monitoring`、
//! `dynamic_monitoring_summary`）。当主库是安装了 `timescaledb` 扩展的
//! `PostgreSQL` 时，本模块把这些时序表转换为 `hypertable`，并按 `chunk`
//! 配置列式压缩与自动保留策略：
//!
//! - **压缩**：对 `compress_after_days` 天前的 `chunk` 启用列式压缩（`zstd`），
//!   监控 JSON 数据通常可压缩 10 倍以上；
//! - **保留**：当 `retention_days > 0` 时，超过该天数的 `chunk` 由后台 job
//!   自动删除（`drop_chunks`），磁盘占用从此有上界；默认 `0` = 不启用，
//!   避免误删历史数据（如从普通 `PostgreSQL` 迁移过来的存量）。
//!
//! 每次服务启动时由 [`crate::init_db_connection`] 调用，全部操作幂等：
//! - 未安装 `timescaledb` 扩展 → 直接跳过（普通 `PostgreSQL` / `SQLite`
//!   不受影响）；
//! - 已是 `hypertable` / 已启用压缩 → 跳过对应转换步骤；
//! - 压缩与保留策略每次启动按配置重新应用（先移除再注册），保证配置变更
//!   在下次启动时生效。
//!
//! # 主键契约变化（重要）
//!
//! `hypertable` 要求所有唯一索引（含主键）包含分区列。因此把表转换为
//! `hypertable` 时，主键从 `(id)` 调整为 **`(id, timestamp)`**，`id` 不再是
//! 唯一约束列（仍为自增 identity 且全局唯一）。
//!
//! - 现有代码（按 `id` 过滤的增删改查、无目标 `ON CONFLICT DO NOTHING`
//!   批量写入）均不受影响；
//! - 但**启用 `TimescaleDB` 后**，任何 `REFERENCES <table>(id)` 外键或
//!   `ON CONFLICT (id)` 语句将不再合法，需要把目标列改为 `(id, timestamp)`。
//! - 该转换**不可逆**：普通 `PostgreSQL`（无 `timescaledb` 扩展）无法读取
//!   已转换的表，降级前必须先 `untable`（官方迁移工具）或备份。
//!
//! # 失败处理
//!
//! 扩展检测或 now 函数创建失败会返回错误（阻断启动，提示环境问题）；
//! 单表转换/策略失败仅记录 error 日志并继续（服务以普通表运行），但会输出
//! 醒目的失败横幅，避免"以为启用了压缩/保留实际没有"的静默降级。
//!
//! # 为什么不在 migration 里做
//!
//! `create_hypertable(..., migrate_data => true)` 不能在事务块内执行，
//! 而 `SeaORM` migration 默认在事务中运行。因此本模块放在 `Migrator::up`
//! 之后的非事务初始化阶段，与 `SQLite` `PRAGMA` 优化属于同一层。

use anyhow::Context;
use ng_core::config::TimescaleConfig;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use tracing::{debug, error, info};

/// 需要转换为 `hypertable` 的时序表。
struct TimescaleTable {
    /// 表名
    name: &'static str,
    /// 默认主键约束名（`<table>_pkey`，由 `SeaORM` 建表时生成）
    pk_name: &'static str,
    /// 压缩时的 `segmentby` 列（按 agent 分片，同一 agent 的连续行压在一起）
    segment_by: &'static str,
}

/// 一天的毫秒数，`chunk` 间隔与策略时长按整数时间列（毫秒 epoch）计算。
const MS_PER_DAY: u64 = 86_400_000;

/// 毫秒 epoch 的 `now()` 函数名，供 `set_integer_now_func` 注册。
const NOW_FUNC: &str = "ng_epoch_ms_now";

/// 参与时序优化的表清单。
const TIMESCALE_TABLES: &[TimescaleTable] = &[
    TimescaleTable {
        name: "dynamic_monitoring",
        pk_name: "dynamic_monitoring_pkey",
        segment_by: "uuid_id",
    },
    TimescaleTable {
        name: "dynamic_monitoring_summary",
        pk_name: "dynamic_monitoring_summary_pkey",
        segment_by: "uuid_id",
    },
];

/// 天 → 毫秒。
///
/// 溢出（超过 `i64::MAX` 毫秒，即 `SQL` `BIGINT` 上限）时返回错误，
/// 由调用方走失败横幅路径——配置错误应显式暴露，而不是 debug panic
/// 或 release 回绕成错误的小数值。
fn days_to_ms(days: u64) -> anyhow::Result<u64> {
    let ms = days.checked_mul(MS_PER_DAY).ok_or_else(|| {
        anyhow::anyhow!("timescale 时长配置过大（{days} 天），超出 SQL BIGINT 毫秒上限")
    })?;
    if i64::try_from(ms).is_err() {
        return Err(anyhow::anyhow!(
            "timescale 时长配置过大（{days} 天），超出 SQL BIGINT 毫秒上限"
        ));
    }
    Ok(ms)
}

/// 为 SQL 标识符加双引号并转义内嵌双引号（防注入）。
///
/// 当前所有入参均为编译期常量，此函数是防御性措施，同时保证 `schema`
/// 限定名拼装正确。
fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// 构造 Timescale 的 `regclass` 文本参数：`'"schema"."name"'`。
///
/// 与 `quote_ident` 一致对标识符加双引号并转义，使含大写/特殊字符的
/// `schema`（`current_schema()` 为运行时值）也能被 `PostgreSQL` 正确解析，
/// 文本内的单引号转义为 `''`。
fn regclass_literal(schema: &str, name: &str) -> String {
    let qualified = format!("{}.{}", quote_ident(schema), quote_ident(name));
    format!("'{}'", qualified.replace('\'', "''"))
}

/// 初始化 `TimescaleDB` 时序优化（幂等）。
///
/// 仅在 `PostgreSQL` 且安装 `timescaledb` 扩展时执行；否则静默跳过，
/// 保证普通部署不受任何影响。
///
/// # Errors
///
/// 当扩展检测或 now 函数创建失败时返回错误（阻断启动，提示环境问题）；
/// 单表转换/策略失败仅记录 error 日志并继续，不影响服务启动。
pub async fn setup_timescale_if_available(
    db: &DatabaseConnection,
    config: Option<&TimescaleConfig>,
) -> anyhow::Result<()> {
    if db.get_database_backend() != DatabaseBackend::Postgres {
        return Ok(());
    }
    if !has_timescale_extension(db).await? {
        debug!(target: "db", "timescaledb extension not installed; skipping timescale setup");
        return Ok(());
    }

    let config = config.cloned().unwrap_or_default();
    info!(
        target: "db",
        chunk_interval_days = config.chunk_interval_days,
        compress_after_days = config.compress_after_days,
        retention_days = config.retention_days,
        "timescaledb detected; applying hypertable setup"
    );

    // 当前连接的默认 schema（search_path 第一个）。所有 DDL / 查询都显式
    // 限定该 schema，消除对 search_path 的隐式依赖。
    let schema = current_schema(db).await?;
    let func_ref = format!("{}.{}", quote_ident(&schema), quote_ident(NOW_FUNC));

    // 创建毫秒 epoch now() 函数（幂等，固定建在当前 schema）。
    // STABLE：事务内返回值一致，满足 TimescaleDB 对 now 函数的要求。
    db.execute_unprepared(&format!(
        "CREATE OR REPLACE FUNCTION {func_ref}() RETURNS BIGINT \
         LANGUAGE SQL STABLE PARALLEL SAFE \
         AS $$ SELECT (extract(epoch FROM now()) * 1000)::BIGINT $$;",
    ))
    .await
    .context("failed to create timescale now() function")?;

    let mut failures = 0;
    for table in TIMESCALE_TABLES {
        if let Err(e) = setup_table(db, table, &config, &schema).await {
            failures += 1;
            error!(target: "db", table = table.name, error = %e, "timescale setup failed for table");
        }
    }
    if failures > 0 {
        // 醒目失败横幅：单表失败不阻断启动（普通表仍可用），但必须让
        // 运维明确知道压缩/保留策略可能未生效，避免静默降级。
        error!(
            target: "db",
            failed = failures,
            total = TIMESCALE_TABLES.len(),
            "TimescaleDB 初始化存在失败：服务将以普通表继续运行，压缩/保留策略可能未生效。\
             请检查上方 error 日志修复后重启；若确认无需 TimescaleDB，可忽略。"
        );
    } else {
        info!(target: "db", "timescale setup completed");
    }
    Ok(())
}

/// 对单个表执行幂等的 `hypertable` 转换与策略配置。
async fn setup_table(
    db: &DatabaseConnection,
    table: &TimescaleTable,
    config: &TimescaleConfig,
    schema: &str,
) -> anyhow::Result<()> {
    let table_ref = format!("{}.{}", quote_ident(schema), quote_ident(table.name));
    let pk_ref = quote_ident(table.pk_name);
    // compress_segmentby 接受列名字符串字面量（非标识符），列名为编译期常量。
    let seg_literal = format!("'{}'", table.segment_by);
    // Timescale 的 regclass 文本参数（create_hypertable / 策略函数），
    // 用 quote_ident 限定 schema（含大写/特殊字符的 schema 也能正确解析）。
    let table_regclass = regclass_literal(schema, table.name);
    let now_regclass = regclass_literal(schema, NOW_FUNC);

    if !is_hypertable(db, table.name).await? {
        // 防御性检查：timestamp 列在迁移定义中为 NOT NULL，正常情况下
        // 不存在 NULL；一旦旧库/脏数据出现 NULL，ADD PRIMARY KEY (id, timestamp)
        // 会永久失败且每次启动重复报错。此处提前检查并给出明确错误。
        let has_null_ts = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                format!("SELECT 1 FROM {table_ref} WHERE timestamp IS NULL LIMIT 1"),
            ))
            .await?;
        if has_null_ts.is_some() {
            anyhow::bail!(
                "table {} contains rows with NULL timestamp; cannot convert to hypertable \
                 (primary key (id, timestamp) requires a non-null timestamp column)",
                table.name
            );
        }

        // hypertable 要求所有唯一索引（含主键）包含分区列 timestamp，
        // 因此先把主键从 (id) 调整为 (id, timestamp)。
        // id 仍为自增 identity 且全局唯一，SeaORM 的按 id 删除不受影响。
        // 注意：pk_name 依赖 SeaORM 建表时生成的默认约束名 `<table>_pkey`；
        // 若主键约束曾被手工改名，此 DROP 不命中、下方 ADD PRIMARY KEY 会
        // 因主键已存在而失败（错误信息会提示，需按实际约束名调整）。
        // 注意：此 DDL 与下方 create_hypertable 分属两个隐式事务，进程
        // 间隙被杀时表会短暂无主键（下次启动可自愈，因为再跑会先检查
        // is_hypertable 再重建主键）。
        db.execute_unprepared(&format!(
            "ALTER TABLE {table_ref} DROP CONSTRAINT IF EXISTS {pk_ref}; \
             ALTER TABLE {table_ref} ADD PRIMARY KEY (id, timestamp);"
        ))
        .await?;

        // migrate_data => true：把表内已有数据迁入 hypertable。
        // 必须在非事务上下文执行，见模块文档。
        // 首次转换会随存量数据量拉长启动时间（一次性成本）。
        db.execute_unprepared(&format!(
            "SELECT create_hypertable({table_regclass}, 'timestamp', \
             chunk_time_interval => {chunk_ms}, migrate_data => true);",
            chunk_ms = days_to_ms(config.chunk_interval_days)?,
        ))
        .await?;
        info!(target: "db", table = table.name, "converted to hypertable");
    }

    // 注册整数时间 now() 函数（幂等：已设置则跳过；重复注册会报错）。
    if !has_integer_now_func(db, table.name).await? {
        db.execute_unprepared(&format!(
            "SELECT set_integer_now_func({table_regclass}, {now_regclass});",
        ))
        .await?;
        info!(target: "db", table = table.name, now_func = %NOW_FUNC, "integer_now_func registered");
    }

    // 压缩设置（仅首次开启；开启后 segmentby/orderby 不可随意变更）。
    if !is_compression_enabled(db, table.name).await? {
        db.execute_unprepared(&format!(
            "ALTER TABLE {table_ref} SET (timescaledb.compress, \
             timescaledb.compress_segmentby = {seg_literal}, \
             timescaledb.compress_orderby = 'timestamp DESC');",
        ))
        .await?;
        info!(target: "db", table = table.name, "compression enabled");
    }

    // 压缩策略：先移除再注册，使配置变更在下次启动时生效（幂等）。
    // compress_after_days = 0 表示所有数据立即可压缩（含实时数据）。
    db.execute_unprepared(&format!(
        "SELECT remove_compression_policy({table_regclass}, if_exists => true); \
         SELECT add_compression_policy({table_regclass}, compress_after => {ms});",
        ms = days_to_ms(config.compress_after_days)?,
    ))
    .await?;
    info!(target: "db", table = table.name, compress_after_days = config.compress_after_days, "compression policy applied");

    if config.retention_days > 0 {
        db.execute_unprepared(&format!(
            "SELECT remove_retention_policy({table_regclass}, if_exists => true); \
             SELECT add_retention_policy({table_regclass}, drop_after => {ms});",
            ms = days_to_ms(config.retention_days)?,
        ))
        .await?;
        info!(target: "db", table = table.name, retention_days = config.retention_days, "retention policy applied");
    } else {
        // 显式关闭：确保不留残留策略（例如从 >0 改回 0）。
        db.execute_unprepared(&format!(
            "SELECT remove_retention_policy({table_regclass}, if_exists => true);",
        ))
        .await?;
        debug!(target: "db", table = table.name, "retention disabled (retention_days = 0)");
    }

    Ok(())
}

/// 是否已安装 `timescaledb` 扩展。
async fn has_timescale_extension(db: &DatabaseConnection) -> anyhow::Result<bool> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM pg_extension WHERE extname = 'timescaledb' LIMIT 1".to_owned(),
        ))
        .await?;
    Ok(row.is_some())
}

/// 当前连接的默认 schema（`search_path` 第一个，且已存在的 schema）。
async fn current_schema(db: &DatabaseConnection) -> anyhow::Result<String> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT current_schema() AS schema".to_owned(),
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("failed to query current_schema()"))?;
    row.try_get::<Option<String>>("", "schema")
        .map_err(anyhow::Error::from)?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("current_schema() returned NULL/empty"))
}

/// 表是否已是 `hypertable`（限定当前 schema，避免多 schema 同名表误判）。
async fn is_hypertable(db: &DatabaseConnection, table: &str) -> anyhow::Result<bool> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(
                "SELECT 1 FROM timescaledb_information.hypertables \
                 WHERE hypertable_name = '{table}' \
                   AND hypertable_schema = current_schema() LIMIT 1"
            ),
        ))
        .await?;
    Ok(row.is_some())
}

/// 表是否已启用压缩（限定当前 schema）。
async fn is_compression_enabled(db: &DatabaseConnection, table: &str) -> anyhow::Result<bool> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(
                "SELECT 1 FROM timescaledb_information.hypertables \
                 WHERE hypertable_name = '{table}' \
                   AND hypertable_schema = current_schema() \
                   AND compression_enabled LIMIT 1"
            ),
        ))
        .await?;
    Ok(row.is_some())
}

/// 时间维度是否已注册 `integer_now_func`（限定当前 schema）。
async fn has_integer_now_func(db: &DatabaseConnection, table: &str) -> anyhow::Result<bool> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(
                "SELECT 1 FROM timescaledb_information.dimensions \
                 WHERE hypertable_name = '{table}' \
                   AND hypertable_schema = current_schema() \
                   AND integer_now_func IS NOT NULL LIMIT 1"
            ),
        ))
        .await?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Database;

    #[test]
    fn days_to_ms_rejects_overflow() {
        // 正常值与零值
        assert_eq!(days_to_ms(0).unwrap(), 0);
        assert_eq!(days_to_ms(1).unwrap(), 86_400_000);
        assert_eq!(days_to_ms(7).unwrap(), 604_800_000);
        // 刚好压线（i64::MAX 毫秒）应通过
        let max_days = i64::MAX as u64 / MS_PER_DAY;
        assert!(days_to_ms(max_days).is_ok());
        // 超过 SQL BIGINT 上限（或乘法溢出）应显式报错，而不是回绕/panic
        assert!(days_to_ms(max_days + 1).is_err());
        assert!(days_to_ms(u64::MAX).is_err());
    }

    #[test]
    fn quote_ident_quotes_and_escapes() {
        assert_eq!(quote_ident("dynamic_monitoring"), "\"dynamic_monitoring\"");
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_ident(""), "\"\"");
    }

    #[test]
    fn regclass_literal_quotes_schema_and_escapes() {
        // 普通 schema：双引号包裹后 PG 解析等价
        assert_eq!(
            regclass_literal("public", "dynamic_monitoring"),
            "'\"public\".\"dynamic_monitoring\"'"
        );
        // 含大写/空格/特殊字符的 schema（current_schema() 运行时值）必须被正确解析
        assert_eq!(
            regclass_literal("My Schema", "Tbl"),
            "'\"My Schema\".\"Tbl\"'"
        );
        // 单引号转义为 ''
        assert_eq!(regclass_literal("a'b", "t"), "'\"a''b\".\"t\"'");
    }

    #[tokio::test]
    async fn setup_skips_non_postgres_silently() {
        // SQLite（内存）不是 PostgreSQL，应直接跳过且不报错，
        // 保证普通部署完全不受影响。
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let res = setup_timescale_if_available(&db, None).await;
        assert!(res.is_ok(), "non-postgres must be skipped: {res:?}");
        let res = setup_timescale_if_available(&db, Some(&TimescaleConfig::default())).await;
        assert!(res.is_ok(), "non-postgres must be skipped: {res:?}");
    }
}

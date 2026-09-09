//! 跨 crate 共享的配置结构体。

use serde::{Deserialize, Serialize};

/// TimescaleDB 时序优化配置（可选）。
///
/// 仅当主库连接指向安装了 `timescaledb` 扩展的 PostgreSQL 时生效；
/// 普通 PostgreSQL / SQLite 部署不受影响。
/// 所有字段均有默认值，因此 `[database.timescale]` 整段可省略。
///
/// ```toml
/// [database]
/// database_url = "postgres://user:pass@host:5432/nodeget"
///
/// [database.timescale]
/// chunk_interval_days = 1    # hypertable 分块间隔（天）
/// compress_after_days = 7    # 超过该天数的 chunk 启用压缩
/// retention_days = 0         # 超过该天数的数据自动删除；0 表示不启用（默认，避免误删历史）
/// ```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TimescaleConfig {
    /// hypertable 分块间隔（天），默认 1。
    #[serde(default = "default_chunk_interval_days")]
    pub chunk_interval_days: u64,
    /// 距离当前时间超过该天数的 chunk 启用列式压缩，默认 7。
    #[serde(default = "default_compress_after_days")]
    pub compress_after_days: u64,
    /// 超过该天数的数据由保留策略自动删除；默认 0 = 不启用（避免误删历史数据）。
    /// 仅当显式配置 > 0 时才注册自动删除策略。
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,
}

const fn default_chunk_interval_days() -> u64 {
    1
}

const fn default_compress_after_days() -> u64 {
    7
}

const fn default_retention_days() -> u64 {
    0
}

impl Default for TimescaleConfig {
    fn default() -> Self {
        Self {
            chunk_interval_days: default_chunk_interval_days(),
            compress_after_days: default_compress_after_days(),
            retention_days: default_retention_days(),
        }
    }
}

//! 跨 crate 共享的配置结构体。

use serde::{Deserialize, Serialize};

/// 默认的 hypertable 分块间隔（小时）。
pub const DEFAULT_CHUNK_INTERVAL_HOURS: u64 = 6;

/// 默认的压缩延迟（小时）：chunk 结束满该小时数后启用列式压缩。
pub const DEFAULT_COMPRESS_AFTER_HOURS: u64 = 12;

/// 一天的小时数（把已废弃的 `*_days` 旧配置键换算成小时）。
const HOURS_PER_DAY: u64 = 24;

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
/// chunk_interval_hours = 6    # hypertable 分块间隔（小时），默认 6
/// compress_after_hours = 12   # chunk 结束满该小时数后启用压缩，默认 12
/// retention_days = 0         # 超过该天数的数据自动删除；0 表示不启用（默认，避免误删历史）
/// ```
///
/// 时间配置以**小时**为粒度：监控数据按秒写入，天级粒度过粗——1 天一个
/// `chunk` 意味着单块可达数 GB，压缩/删除都要整块搬运，容易形成磁盘 I/O
/// 尖峰；小时级 `chunk` 让压缩/保留的粒度与"未压缩数据"的总量都可控
/// （生产库实测：6 小时块的压缩耗时约 85 s，1 天块约 340 s）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct TimescaleConfig {
    /// `hypertable` 分块间隔（小时），默认 [`DEFAULT_CHUNK_INTERVAL_HOURS`]。
    /// 优先于已废弃的 `chunk_interval_days`。
    #[serde(default)]
    pub chunk_interval_hours: Option<u64>,
    /// `chunk` 结束满该小时数后启用列式压缩，默认
    /// [`DEFAULT_COMPRESS_AFTER_HOURS`]；`0` = 已结束的 `chunk` 立即可压缩。
    /// 优先于已废弃的 `compress_after_days`。
    #[serde(default)]
    pub compress_after_hours: Option<u64>,
    /// **已废弃**：分块间隔（天）。为兼容旧配置与 entrypoint 生成的配置文件
    /// 而保留，仅在 `chunk_interval_hours` 未设置时生效（1 天 = 24 小时）。
    #[serde(default)]
    pub chunk_interval_days: Option<u64>,
    /// **已废弃**：压缩延迟（天）。同上，仅在 `compress_after_hours` 未设置时生效。
    #[serde(default)]
    pub compress_after_days: Option<u64>,
    /// 超过该天数的数据由保留策略自动删除；默认 0 = 不启用（避免误删历史数据）。
    /// 仅当显式配置 > 0 时才注册自动删除策略。
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,
}

const fn default_retention_days() -> u64 {
    0
}

impl TimescaleConfig {
    /// 生效的分块间隔（小时）：`chunk_interval_hours` 优先，其次由旧键
    /// `chunk_interval_days` 换算，都没有时用默认值。
    #[must_use]
    pub fn effective_chunk_interval_hours(&self) -> u64 {
        self.chunk_interval_hours
            .or_else(|| {
                self.chunk_interval_days
                    .map(|d| d.saturating_mul(HOURS_PER_DAY))
            })
            .unwrap_or(DEFAULT_CHUNK_INTERVAL_HOURS)
    }

    /// 生效的压缩延迟（小时），优先级同 [`Self::effective_chunk_interval_hours`]。
    #[must_use]
    pub fn effective_compress_after_hours(&self) -> u64 {
        self.compress_after_hours
            .or_else(|| {
                self.compress_after_days
                    .map(|d| d.saturating_mul(HOURS_PER_DAY))
            })
            .unwrap_or(DEFAULT_COMPRESS_AFTER_HOURS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Wrapper {
        timescale: TimescaleConfig,
    }

    fn parse(toml_str: &str) -> TimescaleConfig {
        toml::from_str::<Wrapper>(toml_str)
            .expect("timescale config should parse")
            .timescale
    }

    #[test]
    fn effective_durations_prefer_hours_then_days_then_default() {
        // 全部缺省 → 用默认的 6 小时 / 12 小时
        let cfg = TimescaleConfig::default();
        assert_eq!(
            cfg.effective_chunk_interval_hours(),
            DEFAULT_CHUNK_INTERVAL_HOURS
        );
        assert_eq!(
            cfg.effective_compress_after_hours(),
            DEFAULT_COMPRESS_AFTER_HOURS
        );

        // 旧键（entrypoint 生成的配置）仍生效：1 天 = 24 小时
        let cfg = TimescaleConfig {
            chunk_interval_days: Some(1),
            compress_after_days: Some(7),
            ..Default::default()
        };
        assert_eq!(cfg.effective_chunk_interval_hours(), 24);
        assert_eq!(cfg.effective_compress_after_hours(), 168);

        // 新键优先于旧键，且 0 是合法值（= 已结束的 chunk 立即可压缩）
        let cfg = TimescaleConfig {
            chunk_interval_hours: Some(6),
            chunk_interval_days: Some(30),
            compress_after_hours: Some(0),
            compress_after_days: Some(7),
            ..Default::default()
        };
        assert_eq!(cfg.effective_chunk_interval_hours(), 6);
        assert_eq!(cfg.effective_compress_after_hours(), 0);
    }

    #[test]
    fn toml_accepts_new_hours_keys_and_legacy_days_keys() {
        // 新配置：小时
        let cfg = parse(
            "[timescale]\nchunk_interval_hours = 6\ncompress_after_hours = 12\nretention_days = 7\n",
        );
        assert_eq!(cfg.effective_chunk_interval_hours(), 6);
        assert_eq!(cfg.effective_compress_after_hours(), 12);
        assert_eq!(cfg.retention_days, 7);

        // 旧配置：天（仍可解析，换算成小时）；未写的键走默认值
        let cfg = parse("[timescale]\nchunk_interval_days = 1\ncompress_after_days = 7\n");
        assert_eq!(cfg.effective_chunk_interval_hours(), 24);
        assert_eq!(cfg.effective_compress_after_hours(), 168);
        assert_eq!(cfg.retention_days, 0);

        // 整段省略（只有 [timescale] 表头）
        let cfg = parse("[timescale]\n");
        assert_eq!(cfg, TimescaleConfig::default());
    }
}

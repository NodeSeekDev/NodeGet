//! JS Worker 执行结果查询条件与查询请求体

use serde::{Deserialize, Serialize};

/// JS 执行结果的单条查询条件，多条件组合使用。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsResultQueryCondition {
    /// 按结果 ID 精确匹配
    Id(i64),
    /// 按 JS Worker ID 匹配
    JsWorkerId(i64),
    /// 按 JS Worker 名称匹配
    JsWorkerName(String),
    /// 按运行类型（cron / manual 等）匹配
    RunType(String),
    /// 开始时间范围：[from, to]（Unix 毫秒）
    StartTimeFromTo(i64, i64),
    /// 开始时间下界（Unix 毫秒）
    StartTimeFrom(i64),
    /// 开始时间上界（Unix 毫秒）
    StartTimeTo(i64),
    /// 结束时间范围：[from, to]（Unix 毫秒）
    FinishTimeFromTo(i64, i64),
    /// 结束时间下界（Unix 毫秒）
    FinishTimeFrom(i64),
    /// 结束时间上界（Unix 毫秒）
    FinishTimeTo(i64),
    /// 仅返回执行成功的结果
    IsSuccess,
    /// 仅返回执行失败的结果
    IsFailure,
    /// 仅返回正在运行的结果
    IsRunning,
    /// 限制返回行数
    Limit(u64),
    /// 仅返回最新一条
    Last,
}

/// JS 执行结果查询请求体，包含多条件组合。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsResultDataQuery {
    /// 查询条件列表，各条件之间为 AND 关系
    pub condition: Vec<JsResultQueryCondition>,
}

#[cfg(test)]
mod tests {
    use super::{JsResultDataQuery, JsResultQueryCondition};

    // ── JsResultQueryCondition variants ─────────────────────────────

    #[test]
    fn condition_id() {
        let c = JsResultQueryCondition::Id(42);
        assert_eq!(c, JsResultQueryCondition::Id(42));
        assert_ne!(c, JsResultQueryCondition::Id(99));
    }

    #[test]
    fn condition_js_worker_id() {
        let c = JsResultQueryCondition::JsWorkerId(10);
        assert_eq!(c, JsResultQueryCondition::JsWorkerId(10));
    }

    #[test]
    fn condition_js_worker_name() {
        let c = JsResultQueryCondition::JsWorkerName("cron_job".into());
        assert_eq!(c, JsResultQueryCondition::JsWorkerName("cron_job".into()));
    }

    #[test]
    fn condition_run_type() {
        let c = JsResultQueryCondition::RunType("manual".into());
        assert_eq!(c, JsResultQueryCondition::RunType("manual".into()));
    }

    #[test]
    fn condition_start_time_from_to() {
        let c = JsResultQueryCondition::StartTimeFromTo(1000, 2000);
        assert_eq!(c, JsResultQueryCondition::StartTimeFromTo(1000, 2000));
    }

    #[test]
    fn condition_start_time_from() {
        let c = JsResultQueryCondition::StartTimeFrom(1000);
        assert_eq!(c, JsResultQueryCondition::StartTimeFrom(1000));
    }

    #[test]
    fn condition_start_time_to() {
        let c = JsResultQueryCondition::StartTimeTo(2000);
        assert_eq!(c, JsResultQueryCondition::StartTimeTo(2000));
    }

    #[test]
    fn condition_finish_time_from_to() {
        let c = JsResultQueryCondition::FinishTimeFromTo(3000, 4000);
        assert_eq!(c, JsResultQueryCondition::FinishTimeFromTo(3000, 4000));
    }

    #[test]
    fn condition_finish_time_from() {
        let c = JsResultQueryCondition::FinishTimeFrom(3000);
        assert_eq!(c, JsResultQueryCondition::FinishTimeFrom(3000));
    }

    #[test]
    fn condition_finish_time_to() {
        let c = JsResultQueryCondition::FinishTimeTo(4000);
        assert_eq!(c, JsResultQueryCondition::FinishTimeTo(4000));
    }

    #[test]
    fn condition_status_flags() {
        assert_eq!(JsResultQueryCondition::IsSuccess, JsResultQueryCondition::IsSuccess);
        assert_eq!(JsResultQueryCondition::IsFailure, JsResultQueryCondition::IsFailure);
        assert_eq!(JsResultQueryCondition::IsRunning, JsResultQueryCondition::IsRunning);
        assert_ne!(JsResultQueryCondition::IsSuccess, JsResultQueryCondition::IsFailure);
    }

    #[test]
    fn condition_limit() {
        let c = JsResultQueryCondition::Limit(50);
        assert_eq!(c, JsResultQueryCondition::Limit(50));
    }

    #[test]
    fn condition_last() {
        assert_eq!(JsResultQueryCondition::Last, JsResultQueryCondition::Last);
    }

    // ── JsResultDataQuery ───────────────────────────────────────────

    #[test]
    fn data_query_empty_conditions() {
        let q = JsResultDataQuery { condition: vec![] };
        assert!(q.condition.is_empty());
    }

    #[test]
    fn data_query_multiple_conditions() {
        let q = JsResultDataQuery {
            condition: vec![
                JsResultQueryCondition::JsWorkerName("w".into()),
                JsResultQueryCondition::IsSuccess,
                JsResultQueryCondition::Limit(10),
            ],
        };
        assert_eq!(q.condition.len(), 3);
    }

    #[test]
    fn data_query_eq() {
        let q1 = JsResultDataQuery {
            condition: vec![JsResultQueryCondition::Last],
        };
        let q2 = JsResultDataQuery {
            condition: vec![JsResultQueryCondition::Last],
        };
        assert_eq!(q1, q2);
    }

    // ── Serde round-trip ────────────────────────────────────────────

    #[test]
    fn condition_serde_round_trip() {
        let c = JsResultQueryCondition::StartTimeFromTo(100, 200);
        let json = serde_json::to_string(&c).unwrap();
        let de: JsResultQueryCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(c, de);
    }

    #[test]
    fn data_query_serde_round_trip() {
        let q = JsResultDataQuery {
            condition: vec![
                JsResultQueryCondition::Id(1),
                JsResultQueryCondition::IsFailure,
            ],
        };
        let json = serde_json::to_string(&q).unwrap();
        let de: JsResultDataQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(q, de);
    }

    #[test]
    fn condition_debug() {
        let c = JsResultQueryCondition::JsWorkerName("test".into());
        let d = format!("{c:?}");
        assert!(d.contains("JsWorkerName"));
    }
}

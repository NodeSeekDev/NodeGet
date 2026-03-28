use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsResultQueryCondition {
    Id(i64),
    JsWorkerId(i64),
    JsWorkerName(String),
    StartTimeFromTo(i64, i64),
    StartTimeFrom(i64),
    StartTimeTo(i64),
    FinishTimeFromTo(i64, i64),
    FinishTimeFrom(i64),
    FinishTimeTo(i64),
    IsSuccess,
    IsFailure,
    IsRunning,
    Limit(u64),
    Last,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsResultDataQuery {
    pub condition: Vec<JsResultQueryCondition>,
}

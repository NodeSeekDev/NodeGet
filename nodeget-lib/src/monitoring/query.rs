use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticDataQueryField {
    Cpu,
    System,
    Gpu,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicDataQueryField {
    Cpu,
    Ram,
    Load,
    System,
    Disk,
    Network,
    Gpu,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryCondition {
    Uuid(String),
    TimestampFromTo(i64, i64), // start, end
    TimestampFrom(i64),        // start,
    TimestampTo(i64),          // end
    Last,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StaticDataQuery {
    pub fields: Vec<StaticDataQueryField>,
    pub condition: Vec<QueryCondition>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct DynamicDataQuery {
    pub fields: Vec<DynamicDataQueryField>,
    pub condition: Vec<QueryCondition>,
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
    Call,
    Cron,
    Route,
}

impl RunType {
    #[must_use]
    pub const fn handler_name(&self) -> &'static str {
        match self {
            Self::Call => "onCall",
            Self::Cron => "onCron",
            Self::Route => "onRoute",
        }
    }
}

#[derive(Debug, Clone)]
pub enum JsCodeInput {
    Source(String),
    Bytecode(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePoolWorkerInfo {
    pub script_name: String,
    pub active_requests: usize,
    pub last_used_ms: i64,
    pub idle_ms: i64,
    pub runtime_clean_time_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePoolInfo {
    pub total_workers: usize,
    pub workers: Vec<RuntimePoolWorkerInfo>,
}

pub struct CrontabResult {
    id: i64,

    pub cron_id: i64,
    pub cron_name: String,
    pub special_id: Option<i64>,

    pub run_time: Option<i64>,
    pub success: Option<bool>,
    pub message: Option<String>,
}

use tokio::sync::{Mutex, OnceCell};

static GLOBAL_AGENT: OnceCell<Mutex<ureq::Agent>> = OnceCell::const_new();
static PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub async fn httping_target(target: url::Url) -> Result<std::time::Duration, String> {
    let agent = GLOBAL_AGENT
        .get_or_init(async || {
            let config = ureq::Agent::config_builder()
                .timeout_global(Some(PING_TIMEOUT))
                .build();
            let agent = ureq::Agent::new_with_config(config);
            Mutex::new(agent)
        })
        .await;

    let agent_guard = agent.lock().await;

    let start = std::time::Instant::now();
    match agent_guard.get(target.to_string()).call() {
        Ok(_) => Ok(start.elapsed()),
        Err(e) => Err(format!("Failed to http ping target: {e}")),
    }
}

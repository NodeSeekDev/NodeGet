use log::info;

pub async fn run() {
    super::init_or_skip_super_token().await;
    info!("Initialization completed, exiting.");
}

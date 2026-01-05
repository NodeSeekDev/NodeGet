pub mod autostart_manager;

use std::{env, error::Error};

pub fn app_launch() -> Result<(), Box<dyn Error>> {
    let _ = dotenvy::dotenv();

    let enable_autostart = env::var("NODEGET_AUTOSTART")
        .map(|value| value.trim() == "1")
        .unwrap_or(false);

    if enable_autostart {
        autostart_manager::enable()?;
    }

    Ok(())
}

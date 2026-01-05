use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use std::{env, error::Error};

const APP_NAME: &str = "nodeget.nodeseek.com";

fn get_autostart_instance() -> Result<AutoLaunch, Box<dyn Error>> {
    let app_path = env::current_exe()?.to_string_lossy().into_owned();

    let auto = AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&app_path)
        .build()?;

    Ok(auto)
}

pub fn enable() -> Result<(), Box<dyn Error>> {
    let auto = get_autostart_instance()?;
    auto.enable()?;
    Ok(())
}

pub fn disable() -> Result<(), Box<dyn Error>> {
    let auto = get_autostart_instance()?;
    auto.disable()?;
    Ok(())
}

pub fn is_enabled() -> Result<bool, Box<dyn Error>> {
    let auto = get_autostart_instance()?;
    Ok(auto.is_enabled()?)
}

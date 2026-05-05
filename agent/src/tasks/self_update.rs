use nodeget_lib::self_update::{check_if_update_needed, get_url, replace_binary};

pub async fn self_update(tag: &str) -> bool {
    let current = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("Failed to get current exe path: {e}");
        std::process::exit(1);
    });

    let (current_version,target_version, should_update) = check_if_update_needed(tag);

    if should_update {
        log::info!("Updating from version {}.{}.{} to {}.{}.{}",
            current_version.0, current_version.1, current_version.2,
            target_version.0, target_version.1, target_version.2
        );
    } else {
        log::info!("Current version {}.{}.{} is up to date with target version {}.{}.{}",
            current_version.0, current_version.1, current_version.2,
            target_version.0, target_version.1, target_version.2
        );
        return false;
    }

    let url = get_url(tag).unwrap_or_else(|| {
        log::error!("Failed to get download URL for tag: {tag}");
        String::new()
    });

    log::info!("Downloading update from {url}");

    let client = reqwest::Client::new();
    let response = match client
        .get(&url)
        .header("User-Agent", "curl/8.7.1")
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("Download request failed: {e}");
            return false;
        }
    };

    if !response.status().is_success() {
        log::error!("Download failed with status: {}", response.status());
        return false;
    }

    let bytes
        = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            log::error!("Failed to read response body: {e}");
            return false;
        }
    };

    if bytes.len() < 1024 {
        log::error!("Downloaded file too small ({} bytes), aborting", bytes.len());
        return false;
    }

    log::info!("Downloaded {} bytes ", bytes.len());

    match replace_binary(bytes.to_vec()) {
        true => log::info!("Binary replaced successfully"),
        false => log::error!("Failed to replace binary"),
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        if let Err(e) = std::fs::set_permissions(&current, perms) {
            log::warn!("Failed to set executable permission: {e}");
        }
    }

    log::info!("Binary replaced successfully: {}", current.display());
    true
}

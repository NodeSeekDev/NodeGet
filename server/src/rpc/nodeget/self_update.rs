use crate::token::super_token::check_super_token;
use jsonrpsee::core::RpcResult;
use nodeget_lib::error::NodegetError;
use nodeget_lib::permission::token_auth::TokenOrAuth;

pub async fn self_update(token: String) -> RpcResult<()> {
    let process_logic = async {
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super = check_super_token(&token_or_auth)
            .await
            .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

        if !is_super {
            return Err(NodegetError::PermissionDenied(
                "Permission Denied: Super token required".to_owned(),
            ));
        }
        tracing::debug!(target: "server", "Super token verified for self_update");

        // 1. Fetch latest release tag from GitHub
        let client = reqwest::Client::new();
        let release: serde_json::Value = client
            .get("https://api.github.com/repos/NodeSeekDev/NodeGet/releases/latest")
            .header("User-Agent", "NodeGet-Server")
            .send()
            .await
            .map_err(|e| NodegetError::Other(format!("Failed to fetch latest release: {e}")))?
            .json()
            .await
            .map_err(|e| NodegetError::Other(format!("Failed to parse release response: {e}")))?;

        let tag = release["tag_name"]
            .as_str()
            .ok_or_else(|| NodegetError::Other("Missing tag_name in release response".to_owned()))?;

        tracing::info!(target: "server", tag = tag, "Latest release fetched");

        // 2. Check if update needed
        let (current_version, target_version, should_update) =
            nodeget_lib::self_update::check_if_update_needed(tag);

        if !should_update {
            tracing::info!(
                target: "server",
                current = %format!("{}.{}.{}", current_version.0, current_version.1, current_version.2),
                target = %format!("{}.{}.{}", target_version.0, target_version.1, target_version.2),
                "Server is up to date"
            );
            return Ok(());
        }

        tracing::info!(
            target: "server",
            current = %format!("{}.{}.{}", current_version.0, current_version.1, current_version.2),
            target = %format!("{}.{}.{}", target_version.0, target_version.1, target_version.2),
            "Server update available, downloading..."
        );

        // 3. Get download URL
        let url = nodeget_lib::self_update::get_server_url(tag).ok_or_else(|| {
            NodegetError::Other(format!("Failed to get download URL for tag: {tag}"))
        })?;

        tracing::info!(target: "server", url = %url, "Downloading update");

        // 4. Download binary
        let response = client
            .get(&url)
            .header("User-Agent", "NodeGet-Server")
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| NodegetError::Other(format!("Download request failed: {e}")))?;

        if !response.status().is_success() {
            return Err(NodegetError::Other(format!(
                "Download failed with status: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| NodegetError::Other(format!("Failed to read response body: {e}")))?;

        if bytes.len() < 1024 {
            return Err(NodegetError::Other(format!(
                "Downloaded file too small ({} bytes), aborting",
                bytes.len()
            )));
        }

        tracing::info!(target: "server", size = bytes.len(), "Update downloaded");

        // 5. Replace binary
        if !nodeget_lib::self_update::replace_binary(bytes.to_vec()) {
            return Err(NodegetError::Other("Failed to replace binary".to_owned()));
        }

        // 6. Set executable permission on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let current = std::env::current_exe()
                .map_err(|e| NodegetError::Other(format!("Failed to get current exe path: {e}")))?;
            let perms = std::fs::Permissions::from_mode(0o755);
            if let Err(e) = std::fs::set_permissions(&current, perms) {
                tracing::warn!(target: "server", error = %e, "Failed to set executable permission");
            }
        }

        tracing::info!(target: "server", "Binary replaced successfully, scheduling restart");

        // 7. Spawn delayed restart to allow response to return
        tokio::spawn(async {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            tracing::info!(target: "server", "Restarting server...");
            #[cfg(target_os = "windows")]
            {
                nodeget_lib::self_update::restart_process();
            }
            #[cfg(not(target_os = "windows"))]{
                nodeget_lib::self_update::restart_process_with_exec_v();
            }
        });

        Ok(())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            Err(jsonrpsee::types::ErrorObject::owned(
                e.error_code() as i32,
                format!("{e}"),
                None::<()>,
            ))
        }
    }
}

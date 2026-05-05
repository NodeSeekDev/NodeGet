use super::{NodegetError, RELOAD_NOTIFY, RpcResult, SERVER_CONFIG_PATH, ServerConfig, TokenOrAuth};
use crate::token::super_token::check_super_token;
use std::path::Path;
use tracing::{debug, trace};

// 验证配置文件路径，防止路径遍历攻击
fn validate_config_path(config_path: &str) -> anyhow::Result<&Path> {
    trace!(target: "server", path = %config_path, "validating config path");
    let path = Path::new(config_path);

    // 获取当前工作目录作为允许的基础目录
    let current_dir = std::env::current_dir()
        .map_err(|e| NodegetError::Other(format!("Cannot determine working directory: {e}")))?;

    // 获取规范化路径（解析符号链接和相对路径）
    let canonical_path = path
        .canonicalize()
        .map_err(|e| NodegetError::InvalidInput(format!("Invalid config path: {e}")))?;

    // 验证路径在允许目录内
    if !canonical_path.starts_with(&current_dir) {
        return Err(NodegetError::PermissionDenied(
            "Config path must be within working directory".to_owned(),
        )
        .into());
    }

    // 验证是文件而非目录
    if !canonical_path.is_file() {
        return Err(NodegetError::InvalidInput(
            "Config path must be a regular file".to_owned(),
        )
        .into());
    }

    Ok(path)
}

async fn ensure_super_token(token: &str) -> anyhow::Result<()> {
    trace!(target: "server", "checking super token");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let is_super = check_super_token(&token_or_auth)
        .await
        .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

    if !is_super {
        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Super token required".to_owned(),
        )
        .into());
    }

    Ok(())
}

pub async fn read_config(token: String) -> RpcResult<String> {
    debug!(target: "server", "reading server config");
    let process_logic = async {
        ensure_super_token(&token).await?;
        debug!(target: "server", "Super token verified for read_config");

        let config_path = SERVER_CONFIG_PATH.get().ok_or_else(|| {
            NodegetError::Other("Server config path not initialized".to_owned())
        })?;

        // 验证路径安全性，防止路径遍历
        validate_config_path(config_path)?;
        debug!(target: "server", path = %config_path, "Config path validated for read");

        let file = tokio::fs::read_to_string(config_path)
            .await
            .map_err(|e| NodegetError::Other(format!("Failed to read config file: {e}")))?;
        debug!(target: "server", bytes = file.len(), "Config file read successfully");

        Ok(file)
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}

pub async fn edit_config(token: String, config_string: String) -> RpcResult<bool> {
    debug!(target: "server", config_len = config_string.len(), "editing server config");
    let process_logic = async {
        ensure_super_token(&token).await?;
        debug!(target: "server", "Super token verified for edit_config");

        let _parsed: ServerConfig = toml::from_str(&config_string)
            .map_err(|e| NodegetError::ParseError(format!("Config parse error: {e}")))?;
        debug!(target: "server", "Config string parsed successfully");

        let config_path = SERVER_CONFIG_PATH.get().ok_or_else(|| {
            NodegetError::Other("Server config path not initialized".to_owned())
        })?;

        // 验证路径安全性，防止路径遍历
        validate_config_path(config_path)?;
        debug!(target: "server", path = %config_path, "Config path validated");

        // 使用临时文件+原子重命名，确保写入完整性
        let temp_path = format!("{config_path}.tmp");
        tokio::fs::write(&temp_path, config_string)
            .await
            .map_err(|e| {
                NodegetError::Other(format!("Failed to write temp config file: {e}"))
            })?;
        debug!(target: "server", temp_path = %temp_path, "Temp config file written");

        tokio::fs::rename(&temp_path, config_path)
            .await
            .map_err(|e| {
                // 清理临时文件
                drop(tokio::fs::remove_file(&temp_path));
                NodegetError::Other(format!("Failed to rename config file: {e}"))
            })?;
        debug!(target: "server", "Config file renamed from temp to target");

        if let Some(notify) = RELOAD_NOTIFY.get() {
            notify.notify_one();
            debug!(target: "server", "Config reload notification sent");
        }

        Ok(true)
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = nodeget_lib::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}

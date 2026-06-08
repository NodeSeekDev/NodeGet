//! Server 端配置读写 RPC 实现。
//!
//! 提供 `read_config` 和 `edit_config` 两个 RPC 方法，
//! 均需 super token 认证。`edit_config` 使用原子写入 + 热重载通知。

use crate::config::server::ServerConfig;
use crate::{get_reload_notify, get_server_config_path};
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use ng_core::permission::token_auth::TokenOrAuth;
use std::path::Path;
use tracing::{debug, trace, warn};

/// 验证给定 Token 是否为 Super Token。
///
/// 委托至全局 `PermissionChecker`（ng-core），替代原先的 `CheckSuperTokenFn` fn pointer 注入。
///
/// 内部步骤：
/// 1. 解析原始 Token 为 `TokenOrAuth`
/// 2. 获取全局 PermissionChecker
/// 3. 调用 `check_super_token` 验证，非 super token 则返回权限拒绝错误
async fn ensure_super_token(token: &str) -> anyhow::Result<()> {
    trace!(target: "server", "checking super token");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let checker = ng_core::permission::permission_checker::get_permission_checker()
        .ok_or_else(|| NodegetError::Other("PermissionChecker not initialized".to_owned()))?;

    let is_super = checker
        .check_super_token(&token_or_auth)
        .await
        .map_err(|e| {
            warn!(target: "server", "权限拒绝: Super Token 检查失败: {e}");
            NodegetError::PermissionDenied(format!("{e}"))
        })?;

    if !is_super {
        warn!(target: "server", "权限拒绝: 非 Super Token 尝试访问受保护操作");
        return Err(NodegetError::PermissionDenied(
            "Permission Denied: Super token required".to_owned(),
        )
        .into());
    }

    Ok(())
}

/// 验证配置文件路径安全性，防止路径遍历攻击。
///
/// 内部步骤：
/// 1. 获取当前工作目录作为允许的基础目录
/// 2. 规范化路径（解析符号链接和相对路径）
/// 3. 校验路径在允许目录内
/// 4. 校验目标是文件而非目录
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
        warn!(target: "server", "权限拒绝: 配置路径不在工作目录内: {}", config_path);
        return Err(NodegetError::PermissionDenied(
            "Config path must be within working directory".to_owned(),
        )
        .into());
    }

    // 验证是文件而非目录
    if !canonical_path.is_file() {
        return Err(
            NodegetError::InvalidInput("Config path must be a regular file".to_owned()).into(),
        );
    }

    Ok(path)
}

/// 读取服务器配置文件内容。
///
/// - `token` — Super Token 字符串
/// - 返回配置文件原始文本内容
///
/// 内部步骤：
/// 1. 验证 Super Token
/// 2. 获取全局配置文件路径
/// 3. 验证路径安全性
/// 4. 读取文件内容
///
/// # Errors
///
/// 当 Super Token 验证失败、路径无效、或文件读取失败时返回 RPC 错误
pub async fn read_config(token: String) -> jsonrpsee::core::RpcResult<String> {
    debug!(target: "server", "reading server config");
    let process_logic = async {
        // 1. 验证 Super Token
        ensure_super_token(&token).await?;
        debug!(target: "server", "Super token verified for read_config");

        // 2. 获取全局配置文件路径
        let config_path = get_server_config_path()
            .ok_or_else(|| NodegetError::Other("Server config path not initialized".to_owned()))?;

        // 3. 验证路径安全性，防止路径遍历
        validate_config_path(config_path)?;
        debug!(target: "server", path = %config_path, "Config path validated for read");

        // 4. 读取文件内容
        let file = tokio::fs::read_to_string(config_path)
            .await
            .map_err(|e| NodegetError::Other(format!("Failed to read config file: {e}")))?;
        debug!(target: "server", bytes = file.len(), "Config file read successfully");

        Ok(file)
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            // 将 anyhow 错误转换为 RPC 错误响应
            let nodeget_err = anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}

/// 编辑服务器配置文件（原子写入 + 触发热重载通知）。
///
/// - `token` — Super Token 字符串
/// - `config_string` — 新的配置文件内容（TOML 格式）
/// - 返回 `true` 表示写入成功
///
/// 内部步骤：
/// 1. 验证 Super Token
/// 2. 预解析配置字符串，验证 TOML 格式正确性
/// 3. 获取全局配置文件路径并验证安全性
/// 4. 原子写入：先写临时文件，再 rename 覆盖原文件
/// 5. 触发热重载通知
///
/// # Errors
///
/// 当 Super Token 验证失败、配置解析失败、路径无效、或写入失败时返回 RPC 错误
pub async fn edit_config(token: String, config_string: String) -> jsonrpsee::core::RpcResult<bool> {
    debug!(target: "server", config_len = config_string.len(), "editing server config");
    let process_logic = async {
        // 1. 验证 Super Token
        ensure_super_token(&token).await?;
        debug!(target: "server", "Super token verified for edit_config");

        // 2. 预解析配置字符串，验证 TOML 格式正确性（不使用解析结果）
        let _parsed: ServerConfig = toml::from_str(&config_string)
            .map_err(|e| NodegetError::ParseError(format!("Config parse error: {e}")))?;
        debug!(target: "server", "Config string parsed successfully");

        // 3. 获取全局配置文件路径并验证安全性
        let config_path = get_server_config_path()
            .ok_or_else(|| NodegetError::Other("Server config path not initialized".to_owned()))?;

        // 验证路径安全性，防止路径遍历
        validate_config_path(config_path)?;
        debug!(target: "server", path = %config_path, "Config path validated");

        // 4. 原子写入：先写临时文件，再 rename 覆盖原文件
        // 附加随机 UUID 后缀防止并发 edit_config 覆盖彼此的临时文件
        let temp_path = format!("{config_path}.tmp.{}", uuid::Uuid::new_v4());
        tokio::fs::write(&temp_path, config_string)
            .await
            .map_err(|e| NodegetError::Other(format!("Failed to write temp config file: {e}")))?;
        debug!(target: "server", temp_path = %temp_path, "Temp config file written");

        if let Err(e) = tokio::fs::rename(&temp_path, config_path).await {
            // 清理临时文件（await 确保实际执行删除）
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(NodegetError::Other(format!("Failed to rename config file: {e}")).into());
        }
        debug!(target: "server", "Config file renamed from temp to target");

        // 5. 触发热重载通知
        if let Some(notify) = get_reload_notify() {
            notify.notify_one();
            debug!(target: "server", "Config reload notification sent");
        }

        Ok(true)
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            // 将 anyhow 错误转换为 RPC 错误响应
            let nodeget_err = anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}

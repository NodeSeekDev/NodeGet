//! `token_list_all_tokens` RPC 方法实现。
//!
//! 列出所有令牌信息，仅超级令牌可调用。

use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::data_structure::Token;
use ng_core::permission::token_auth::TokenOrAuth;
use serde::Serialize;
use serde_json::value::RawValue;
use tracing::{debug, warn};

use crate::cache::TokenCache;
use crate::super_token::check_super_token;

/// 列出所有令牌的响应结构体。
#[derive(Serialize)]
struct ListAllTokensResponse {
    /// 所有令牌信息列表
    tokens: Vec<Token>,
}

/// 列出所有令牌信息（仅超级令牌可调用）。
///
/// - `token`：超级令牌凭据（用于鉴权）
/// - 返回：`{"tokens":[...]}` 格式的所有 Token 信息
/// - 错误：鉴权失败
///
/// 内部步骤：
/// 1. 验证调用者为超级令牌
/// 2. 从 TokenCache 获取所有条目
/// 3. 将 CachedToken 转换为 Token 结构体并序列化
pub async fn list_all_tokens(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "token", "processing list all tokens request");
        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let is_super_token = check_super_token(&token_or_auth).await.map_err(|e| {
            warn!(target: "token", "权限拒绝: {e}");
            NodegetError::PermissionDenied(format!("{e}"))
        })?;

        if !is_super_token {
            warn!(target: "token", "non-supertoken attempted to list all tokens");
            return Err(NodegetError::PermissionDenied(
                "Only SuperToken can list all tokens".to_owned(),
            )
            .into());
        }

        let cached_tokens = TokenCache::global()
            .ok_or_else(|| NodegetError::ConfigNotFound("TokenCache not initialized".to_owned()))?
            .get_all();

        let tokens: Vec<Token> = cached_tokens
            .iter()
            .map(|entry| Token {
                version: entry.model.version,
                token_key: entry.model.token_key.clone(),
                timestamp_from: entry.model.time_stamp_from,
                timestamp_to: entry.model.time_stamp_to,
                token_limit: entry.parsed_limits.clone(),
                username: entry.model.username.clone(),
            })
            .collect();

        let response = ListAllTokensResponse { tokens };

        debug!(target: "token", token_count = response.tokens.len(), "list_all_tokens completed");
        serde_json::value::to_raw_value(&response)
            .map_err(|e| NodegetError::from(e).into())
    };

    // 统一错误转换：anyhow → NodegetError → JSON-RPC ErrorObject
    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = ng_core::error::anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}

//! `token_get` RPC 方法实现。
//!
//! 查询 Token 信息，支持两种模式：
//! - 普通模式：使用自身凭据查询自己的信息
//! - 超级令牌模式：使用超级令牌凭据查询任意 Token（支持按 key/username 查询）

use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::token_auth::TokenOrAuth;
use serde_json::value::RawValue;
use tracing::{debug, warn};

use crate::get::{get_token, get_token_by_key_or_username};
use crate::super_token::check_super_token;

/// 查询 Token 信息。
///
/// - `token`：待查询的凭据字符串；
///   普通模式下为 `key:secret` 格式，超级令牌模式下可为 token_key 或 username
/// - `supertoken`：可选的超级令牌凭据；
///   提供时进入超级令牌管理模式，允许按 key/username 查询任意 Token
/// - 返回：Token 信息的 JSON RawValue
/// - 错误：凭据无效、超级令牌验证失败
///
/// 内部步骤：
/// 1. 若提供 supertoken，验证其为超级令牌
/// 2. 超级令牌模式下：先尝试将 token 解析为凭据，失败则按 key/username 查询
/// 3. 普通模式下：解析 token 为凭据并认证查询
pub async fn get(token: String, supertoken: Option<String>) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "token", has_supertoken = supertoken.is_some(), "processing token get request");
        let token_info = if let Some(supertoken) = supertoken {
            let supertoken_or_auth = TokenOrAuth::from_full_token(&supertoken).map_err(|e| {
                NodegetError::ParseError(format!("Failed to parse supertoken: {e}"))
            })?;

            let is_super_token = check_super_token(&supertoken_or_auth)
                .await
                .map_err(|e| NodegetError::PermissionDenied(format!("{e}")))?;

            if !is_super_token {
                warn!(target: "token", "non-supertoken attempted supertoken-only get query");
                return Err(NodegetError::PermissionDenied(
                    "Only SuperToken can query by username/token_key in token_get".to_owned(),
                )
                .into());
            }

            match TokenOrAuth::from_full_token(&token) {
                Ok(token_or_auth) => get_token(&token_or_auth).await?,
                Err(_) => get_token_by_key_or_username(&token).await?,
            }
        } else {
            let token_or_auth = TokenOrAuth::from_full_token(&token)
                .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;
            get_token(&token_or_auth).await?
        };

        serde_json::value::to_raw_value(&token_info)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
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

//! `token_create` RPC 方法实现。
//!
//! 创建子令牌，需提供父级超级令牌凭据。

use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_core::permission::create::TokenCreationRequest;
use ng_core::permission::token_auth::TokenOrAuth;
use serde_json::value::RawValue;
use tracing::debug;

use crate::generate_token::generate_and_store_token;

/// 创建子令牌，需提供父级超级令牌凭据。
///
/// - `father_token`：父级令牌凭据（`key:secret` 或 `username|password` 格式），必须是超级令牌
/// - `token_creation`：创建请求参数（时间范围、权限列表、可选 username/password）
/// - 返回：成功时为 `{"key":"<token_key>","secret":"<token_secret>"}`
/// - 错误：凭据解析失败、父级令牌非超级令牌、数据库错误
///
/// 内部步骤：
/// 1. 解析父级凭据为 TokenOrAuth
/// 2. 委托 `generate_and_store_token` 执行实际创建与存储
/// 3. 序列化结果为 RawValue 返回
pub async fn create(
    father_token: String,
    token_creation: TokenCreationRequest,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        let father_token_or_auth = TokenOrAuth::from_full_token(&father_token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        debug!(target: "token", has_username = token_creation.username.is_some(), "Token creation request parsed, verifying super token");

        let (key, secret) = generate_and_store_token(
            &father_token_or_auth,
            token_creation.timestamp_from,
            token_creation.timestamp_to,
            token_creation.token_limit,
            token_creation.username,
            token_creation.password,
        )
        .await?;

        debug!(target: "token", token_key = %key, "Token created successfully");

        serde_json::value::to_raw_value(&serde_json::json!({
            "key": key,
            "secret": secret
        }))
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

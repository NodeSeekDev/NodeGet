//! `crontab.set_enable` RPC 实现：启用或禁用定时任务。

use crate::rpc::crontab::CrontabRpcImpl;
use crate::server_cron::set_crontab_enable_by_name;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use ng_core::permission::data_structure::{Crontab as CrontabPermission, Permission};
use ng_core::permission::token_auth::TokenOrAuth;
use ng_db::entity::crontab;
use ng_infra::server::RpcHelper;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::value::RawValue;
use tracing::{debug, warn};

/// 设置定时任务的启用/禁用状态。
///
/// 1. 解析 Token 格式
/// 2. 查询数据库确认任务存在
/// 3. 解析 CronType 并检查写入权限
/// 4. 调用 `set_crontab_enable_by_name` 更新状态并刷新缓存
///
/// - `token` - 认证 Token 字符串
/// - `name` - 定时任务名称
/// - `enable` - 目标启用状态
/// - 返回 `{"success": true, "enabled": <实际启用状态>}`
pub async fn set_enable(token: String, name: String, enable: bool) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "crontab", name = %name, enable = enable, "processing crontab set_enable request");
        // 0. 校验 name 合法性（低成本操作，最先执行）
        super::validate_name(&name)?;

        let token_or_auth = TokenOrAuth::from_full_token(&token)
            .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

        let db = CrontabRpcImpl::get_db()?;

        // 查询目标任务
        let model = crontab::Entity::find()
            .filter(crontab::Column::Name.eq(&name))
            .one(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let Some(model) = model else {
            warn!(target: "crontab", name = %name, "未找到: Crontab 不存在");
            return Err(NodegetError::NotFound(format!("Crontab not found: {name}")).into());
        };

        debug!(target: "crontab", id = model.id, name = %name, "Crontab found for enable toggle");

        // 解析 CronType 并检查写入权限
        let cron_type = super::auth::parse_cron_type(&model.cron_type, &name)?;
        super::auth::ensure_crontab_scope_permission(
            &token_or_auth,
            &cron_type,
            Permission::Crontab(CrontabPermission::Write),
            "Permission Denied: Missing Crontab Write permission for all target scopes",
        )
        .await?;

        debug!(target: "crontab", name = %name, enable = enable, "Crontab set_enable permission check passed");

        // 执行启用/禁用（内部会刷新缓存）
        let result_state = set_crontab_enable_by_name(name.clone(), enable)
            .await
            .map_err(|e| NodegetError::Other(format!("Failed to set crontab enable: {e}")))?;

        let state = result_state.ok_or_else(|| {
            warn!(target: "crontab", name = %name, "未找到: Crontab 不存在");
            NodegetError::NotFound(format!("Crontab not found: {name}"))
        })?;

        debug!(target: "crontab", name = %name, enabled = state, "Crontab enable state updated");

        serde_json::value::to_raw_value(&serde_json::json!({"success": true, "enabled": state}))
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

    match process_logic.await {
        Ok(result) => Ok(result),
        Err(e) => {
            let nodeget_err = anyhow_to_nodeget_error(&e);
            Err(jsonrpsee::types::ErrorObject::owned(
                nodeget_err.error_code() as i32,
                format!("{nodeget_err}"),
                None::<()>,
            ))
        }
    }
}

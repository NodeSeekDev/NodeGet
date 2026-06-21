//! 权限校验 —— JS Worker 和 JS Result 的 RBAC 权限检查。
//!
//! 提供一系列权限校验辅助函数：
//! - `check_js_worker_permission` —— 检查 Worker 级别权限
//! - `check_get_rt_pool_permission` —— 检查运行时池查看权限
//! - `filter_workers_by_list_permission` —— 按列表权限过滤可见 Worker
//! - `ensure_js_result_permission` —— 检查 Result 级别权限
//! - `resolve_accessible_js_result_workers` —— 解析可访问的 Result Worker 列表
//!
//! 权限校验委托至全局 `ng_core::permission::permission_checker::PermissionChecker`。

use ng_core::error::NodegetError;
use ng_core::permission::data_structure::{
    JsResult as JsResultPermission, JsWorker as JsWorkerPermission, NodeGet, Permission, Scope,
    Token,
};
use ng_core::permission::permission_checker::require_permission_checker as get_checker;
use ng_core::permission::token_auth::TokenOrAuth;
use ng_core::utils::get_local_timestamp_ms_i64;
use ng_db::entity::js_result;
use sea_orm::{EntityTrait, QueryOrder, QuerySelect};
use tracing::{trace, warn};

// ── js_worker 权限校验辅助函数 ────────────────────────────────────

/// 检查 Token 是否在有效期内（`timestamp_from` / `timestamp_to`）。
///
/// 与 `check_token_limit` 内部的时间检查一致。供列表过滤路径（`get_token` +
/// `check_limits_cover` 批量匹配）补齐时间有效性判断，避免过期/未生效 token
/// 因纯内存匹配通过而被放行。返回 `false` 时调用方应视为无权限。
fn token_time_valid(token: &Token) -> bool {
    let Ok(now) = get_local_timestamp_ms_i64() else {
        warn!(target: "js_worker", "failed to read local timestamp, denying by time check");
        return false;
    };
    if let Some(from) = token.timestamp_from
        && now < from
    {
        warn!(target: "auth", token_key = %token.token_key, "token not yet valid (timestamp_from)");
        return false;
    }
    if let Some(to) = token.timestamp_to
        && now > to
    {
        warn!(target: "auth", token_key = %token.token_key, "token expired (timestamp_to)");
        return false;
    }
    true
}

/// 检查 token 是否具有指定 Worker 的指定权限。
///
/// - `token` —— 完整 token 字符串（key:secret 或 username|password）
/// - `worker_name` —— Worker 名称
/// - `permission` —— 所需权限（Create/Read/Write/Delete/Run/...）
///
/// 内部步骤：
/// 1. 解析 token 为 `TokenOrAuth`
/// 2. 调用 `check_token_limit` 检查 `Scope::JsWorker(worker_name)` + `Permission::JsWorker(permission)`
/// 3. 不通过则返回 `PermissionDenied` 错误
pub async fn check_js_worker_permission(
    token: &str,
    worker_name: &str,
    permission: JsWorkerPermission,
) -> anyhow::Result<()> {
    trace!(target: "js_worker", worker_name = %worker_name, permission = ?permission, "checking js_worker permission");
    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let permission_name = format!("{permission:?}");
    let is_allowed = checker
        .check_token_limit(
            &token_or_auth,
            &[Scope::JsWorker(worker_name.to_owned())],
            &[Permission::JsWorker(permission)],
        )
        .await?;

    if is_allowed {
        return Ok(());
    }

    warn!(target: "js_worker", worker_name = %worker_name, permission = %permission_name, "permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "Permission denied for js_worker '{worker_name}', required permission: {permission_name}"
    ))
    .into())
}

/// 检查 token 是否具有运行时池查看权限（`nodeget.get_rt_pool`）。
///
/// 此权限属于 `Scope::Global` + `Permission::NodeGet(GetRtPool)`。
pub async fn check_get_rt_pool_permission(token: &str) -> anyhow::Result<()> {
    trace!(target: "js_worker", "checking get_rt_pool permission");
    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let is_allowed = checker
        .check_token_limit(
            &token_or_auth,
            &[Scope::Global],
            &[Permission::NodeGet(NodeGet::GetRtPool)],
        )
        .await?;

    if is_allowed {
        return Ok(());
    }

    warn!(target: "js_worker", "get_rt_pool permission denied");
    Err(NodegetError::PermissionDenied(
        "Permission denied: missing nodeget.get_rt_pool permission".to_owned(),
    )
    .into())
}

/// 按列表权限过滤 Worker 名称，仅返回 token 有权查看的 Worker。
///
/// - `token` —— 完整 token 字符串
/// - `worker_names` —— 待过滤的 Worker 名称列表
///
/// 优化：先短路超级令牌，否则仅 `get_token` 一次，对每个 worker_name 用
/// `check_limits_cover` 做纯内存匹配（同步、无认证/无 DB），避免 N 次串行
/// `check_token_limit`（每次内部重复 `get_token` 重新 ct_eq 验证 + clone limits）。
pub async fn filter_workers_by_list_permission(
    token: &str,
    worker_names: Vec<String>,
) -> anyhow::Result<Vec<String>> {
    trace!(target: "js_worker", count = worker_names.len(), "filtering workers by list permission");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    // 超级令牌直接放行全部
    if ng_token::check_super_token(&token_or_auth).await? {
        return Ok(worker_names);
    }

    // 仅认证一次，拿到 token_limit（Arc<Vec<Limit>>，取切片无 clone）
    let token_info = ng_token::get_token(&token_or_auth).await?;
    // 补齐时间有效性检查（与 check_token_limit 一致），过期/未生效则视为无权限
    if !token_time_valid(&token_info) {
        return Ok(Vec::new());
    }
    let limits = token_info.token_limit.as_slice();
    let required_perm = Permission::JsWorker(JsWorkerPermission::ListAllJsWorker);

    // 纯内存匹配，收集允许的子集
    let mut allowed = Vec::new();
    for worker_name in worker_names {
        let is_allowed = ng_token::get::check_limits_cover(
            limits,
            &Scope::JsWorker(worker_name.clone()),
            &required_perm,
        );

        if is_allowed {
            allowed.push(worker_name);
        }
    }

    Ok(allowed)
}

// ── js_result 权限校验辅助函数 ────────────────────────────────────

/// JS Result 操作类型。
#[derive(Debug, Clone, Copy)]
pub enum JsResultAction {
    /// 读取结果
    Read,
    /// 删除结果
    Delete,
}

/// 根据 action 和 worker_name 构建所需的 Permission。
fn build_required_permission(action: JsResultAction, worker_name: &str) -> Permission {
    match action {
        JsResultAction::Read => {
            Permission::JsResult(JsResultPermission::Read(worker_name.to_owned()))
        }
        JsResultAction::Delete => {
            Permission::JsResult(JsResultPermission::Delete(worker_name.to_owned()))
        }
    }
}

/// 检查 token 是否具有指定 Worker 的 Result 操作权限。
///
/// - `token` —— 完整 token 字符串
/// - `worker_name` —— Worker 名称
/// - `action` —— 操作类型（Read/Delete）
pub async fn ensure_js_result_permission(
    token: &str,
    worker_name: &str,
    action: JsResultAction,
) -> anyhow::Result<()> {
    trace!(target: "js_result", worker_name = %worker_name, action = ?action, "checking js_result permission");
    let checker = get_checker()?;
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let is_allowed = checker
        .check_token_limit(
            &token_or_auth,
            &[Scope::JsWorker(worker_name.to_owned())],
            &[build_required_permission(action, worker_name)],
        )
        .await?;

    if is_allowed {
        return Ok(());
    }

    warn!(target: "js_result", worker_name = %worker_name, action = ?action, "permission denied");
    Err(NodegetError::PermissionDenied(format!(
        "Permission denied for js_result on worker '{worker_name}', action: {action:?}"
    ))
    .into())
}

/// 解析 token 可访问的 Result Worker 列表。
///
/// 从数据库查询所有 `js_result` 记录的 `js_worker_name`（去重），
/// 然后检查权限，返回允许访问的子集。
///
/// - `token` —— 完整 token 字符串
/// - `action` —— 操作类型（Read/Delete）
///
/// 优化：先短路超级令牌（直接返回全部 worker_names），否则仅 `get_token` 一次，
/// 对每个 worker_name 用 `check_limits_cover` 做纯内存匹配（同步、无认证/无 DB），
/// 避免 N 次串行 `check_token_limit`（每次内部重复 `get_token` 重新 ct_eq 验证 + clone limits）。
pub async fn resolve_accessible_js_result_workers(
    token: &str,
    action: JsResultAction,
) -> anyhow::Result<Vec<String>> {
    trace!(target: "js_result", action = ?action, "resolving accessible js_result workers");
    let token_or_auth = TokenOrAuth::from_full_token(token)
        .map_err(|e| NodegetError::ParseError(format!("Failed to parse token: {e}")))?;

    let db = ng_db::get_db()
        .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;

    // 查询所有 js_result 记录中的 js_worker_name 并去重
    let mut worker_names: Vec<String> = js_result::Entity::find()
        .select_only()
        .column(js_result::Column::JsWorkerName)
        .order_by_asc(js_result::Column::JsWorkerName)
        .into_tuple()
        .all(db)
        .await
        .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

    worker_names.dedup();

    // 超级令牌直接放行全部
    if ng_token::check_super_token(&token_or_auth).await? {
        return Ok(worker_names);
    }

    // 仅认证一次，拿到 token_limit（Arc<Vec<Limit>>，取切片无 clone）
    let token_info = ng_token::get_token(&token_or_auth).await?;
    // 补齐时间有效性检查（与 check_token_limit 一致），过期/未生效则视为无权限
    if !token_time_valid(&token_info) {
        return Ok(Vec::new());
    }
    let limits = token_info.token_limit.as_slice();

    // 纯内存匹配，保留允许的 Worker
    let mut allowed = Vec::new();
    for worker_name in worker_names {
        let required_perm = build_required_permission(action, worker_name.as_str());
        let is_allowed = ng_token::get::check_limits_cover(
            limits,
            &Scope::JsWorker(worker_name.clone()),
            &required_perm,
        );

        if is_allowed {
            allowed.push(worker_name);
        }
    }

    Ok(allowed)
}

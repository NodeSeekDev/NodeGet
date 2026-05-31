use crate::js_worker::auth::filter_workers_by_list_permission;
use jsonrpsee::core::RpcResult;
use ng_core::error::NodegetError;
use ng_db::entity::js_worker;
use ng_db::get_db;
use sea_orm::{EntityTrait, QueryOrder, QuerySelect};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn list_all_js_worker(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "js_worker", "processing list all js_worker request");
        let db =
            get_db().ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()))?;
        let all_names: Vec<String> = js_worker::Entity::find()
            .select_only()
            .column(js_worker::Column::Name)
            .order_by_asc(js_worker::Column::Name)
            .into_tuple()
            .all(db)
            .await
            .map_err(|e| NodegetError::DatabaseError(e.to_string()))?;

        let allowed_names = filter_workers_by_list_permission(&token, all_names).await?;

        debug!(target: "js_worker", count = allowed_names.len(), "list_all_js_worker completed");

        let json_str = serde_json::to_string(&allowed_names)
            .map_err(|e| NodegetError::SerializationError(e.to_string()))?;
        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(e.to_string()).into())
    };

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

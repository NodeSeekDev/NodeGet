use crate::auth::{KvNamespaceListPermission, resolve_kv_list_namespace_permission};
use crate::db::list_all_namespaces;
use jsonrpsee::core::RpcResult;
use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
use serde_json::value::RawValue;
use tracing::debug;

pub async fn list_all_namespace(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!(target: "kv", "Processing list_all_namespace request");

        let permission = resolve_kv_list_namespace_permission(&token).await?;
        debug!(target: "kv", "list_all_namespace permission check passed");
        let namespaces = list_all_namespaces().await?;

        let filtered_namespaces = match permission {
            KvNamespaceListPermission::All => namespaces,
            KvNamespaceListPermission::Scoped(allowed) => namespaces
                .into_iter()
                .filter(|namespace| allowed.contains(namespace))
                .collect(),
        };

        debug!(target: "kv", namespace_count = filtered_namespaces.len(), "list_all_namespace completed");

        let json_str = serde_json::to_string(&filtered_namespaces).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize namespaces: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
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

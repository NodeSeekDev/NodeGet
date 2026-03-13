use crate::kv::list_all_namespaces;
use crate::rpc::kv::auth::{KvNamespaceListPermission, resolve_kv_list_namespace_permission};
use jsonrpsee::core::RpcResult;
use log::debug;
use nodeget_lib::error::NodegetError;
use serde_json::value::RawValue;

pub async fn list_all_namespace(token: String) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        debug!("KV RPC: Processing list_all_namespace request");

        let permission = resolve_kv_list_namespace_permission(&token).await?;
        let namespaces = list_all_namespaces().await?;

        let filtered_namespaces = match permission {
            KvNamespaceListPermission::All => namespaces,
            KvNamespaceListPermission::Scoped(allowed) => namespaces
                .into_iter()
                .filter(|namespace| allowed.contains(namespace))
                .collect(),
        };

        let json_str = serde_json::to_string(&filtered_namespaces).map_err(|e| {
            NodegetError::SerializationError(format!("Failed to serialize namespaces: {e}"))
        })?;

        RawValue::from_string(json_str)
            .map_err(|e| NodegetError::SerializationError(format!("{e}")).into())
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

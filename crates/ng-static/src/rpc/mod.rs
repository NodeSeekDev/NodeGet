pub mod static_bucket;
pub mod static_bucket_file;

use static_bucket::RpcServer as _;
use static_bucket_file::RpcServer as _;

/// Build and return an `RpcModule` containing all static bucket RPC methods.
///
/// The caller should merge this into the main RPC module during startup.
pub fn rpc_module() -> jsonrpsee::RpcModule<()> {
    let mut module = jsonrpsee::RpcModule::new(());
    module
        .merge(static_bucket::StaticBucketRpcImpl.into_rpc())
        .expect("Failed to merge static-bucket RPC");
    module
        .merge(static_bucket_file::StaticBucketFileRpcImpl.into_rpc())
        .expect("Failed to merge static-bucket-file RPC");
    module
}

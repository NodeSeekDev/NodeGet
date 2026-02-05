use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use log::info;
use nodeget_lib::utils::version::NodeGetVersion;
use serde_json::Value;

// NodeGet 服务端基础功能 RPC 接口定义
#[rpc(server, namespace = "nodeget-server")]
pub trait Rpc {
    // 服务健康检查方法，返回服务运行状态
    #[method(name = "hello")]
    async fn hello(&self) -> String;

    // 获取服务版本信息方法，返回当前服务的版本信息
    #[method(name = "version")]
    async fn version(&self) -> Value;
}

// NodeGet 服务端 RPC 实现结构体
pub struct NodegetServerRpcImpl;

#[async_trait]
impl RpcServer for NodegetServerRpcImpl {
    // 服务健康检查实现
    //
    // # 返回值
    // 返回 "NodeGet Server Is Running!" 字符串表示服务正常运行
    async fn hello(&self) -> String {
        info!("Hello Request");
        "NodeGet Server Is Running!".to_string()
    }

    // 获取服务版本信息实现
    //
    // # 返回值
    // 返回当前服务版本信息的 JSON 值
    async fn version(&self) -> Value {
        info!("Version Request");
        serde_json::to_value(NodeGetVersion::get()).unwrap()
    }
}

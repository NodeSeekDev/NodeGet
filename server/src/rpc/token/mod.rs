// 令牌创建模块
mod create;
// 令牌获取模块
mod get;

use jsonrpsee::proc_macros::rpc;
use migration::async_trait::async_trait;
use nodeget_lib::permission::create::TokenCreationRequest;
use serde_json::Value;

// 令牌管理相关的 RPC 接口定义，包括获取和创建令牌功能
#[rpc(server, namespace = "token")]
pub trait Rpc {
    // 获取令牌信息方法
    // 
    // # 参数
    // * `token` - 认证令牌
    // 
    // # 返回值
    // 返回令牌信息
    #[method(name = "get")]
    async fn get(&self, token: String) -> Value;

    // 创建新令牌方法
    // 
    // # 参数
    // * `father_token` - 父级令牌
    // * `token_creation` - 令牌创建请求参数
    // 
    // # 返回值
    // 返回创建的令牌信息
    #[method(name = "create")]
    async fn create(&self, father_token: String, token_creation: TokenCreationRequest) -> Value;
}
// 令牌管理 RPC 实现结构体
pub struct TokenRpcImpl;

#[async_trait]
impl RpcServer for TokenRpcImpl {
    // 获取令牌信息实现
    // 
    // # 参数
    // * `token` - 认证令牌
    // 
    // # 返回值
    // 返回令牌信息
    async fn get(&self, token: String) -> Value {
        get::get(token).await
    }

    // 创建新令牌实现
    // 
    // # 参数
    // * `father_token` - 父级令牌
    // * `token_creation` - 令牌创建请求参数
    // 
    // # 返回值
    // 返回创建的令牌信息
    async fn create(&self, father_token: String, token_creation: TokenCreationRequest) -> Value {
        create::create(father_token, token_creation).await
    }
}

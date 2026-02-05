use crate::DB;
use sea_orm::{ActiveValue, DatabaseConnection, Set};
use serde::Serialize;
use serde_json::{Value, to_value};

// Agent 相关 RPC 接口模块
pub mod agent;
// NodeGet 服务端基础功能 RPC 接口模块
pub mod nodeget;
// 任务管理 RPC 接口模块
pub mod task;
// 令牌管理 RPC 接口模块
pub mod token;

pub mod metadata;

// RPC 辅助功能 trait，提供数据库操作和序列化工具方法
pub trait RpcHelper {
    // 尝试将值序列化为 JSON 并包装为 ActiveValue
    //
    // # 参数
    // * `val` - 需要序列化的值
    //
    // # 返回值
    // 成功返回包装后的 ActiveValue，失败返回错误消息
    fn try_set_json<T: Serialize>(val: T) -> Result<ActiveValue<Value>, String> {
        to_value(val)
            .map(Set)
            .map_err(|e| format!("Serialization error: {e}"))
    }

    // 获取数据库连接引用
    //
    // # 返回值
    // 成功返回数据库连接引用，失败返回错误代码和消息
    fn get_db() -> Result<&'static DatabaseConnection, (i64, String)> {
        DB.get()
            .ok_or_else(|| (102, "DB not initialized".to_string()))
    }
}

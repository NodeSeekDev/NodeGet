use sha2::{Digest, Sha256};

// 令牌生成模块
pub mod generate_token;
// 令牌获取和权限检查模块
pub mod get;
// 超级令牌管理模块
pub mod super_token;

// 对字符串进行 SHA256 哈希计算，并添加 "NODEGET" 前缀
//
// # 参数
// * `need_hash` - 需要哈希的字符串
//
// # 返回值
// 返回 SHA256 哈希值的十六进制字符串表示
pub fn hash_string(need_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("NODEGET{need_hash}"));
    hex::encode(hasher.finalize())
}

use crate::DB;
use crate::entity::token;
use sea_orm::{ColumnTrait, DeleteResult, EntityTrait, QueryFilter};

// 删除令牌的方法
//
// # 参数
// * `token_key` - 要删除的令牌的 key
//
// # 返回值
// 返回删除结果，包含删除的行数，或数据库错误
pub async fn delete_token_by_key(token_key: String) -> Result<DeleteResult, sea_orm::DbErr> {
    let Some(db) = DB.get() else {
            return Err(sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(
                "Database not initialized".to_owned(),
            )));
        };

    // 根据 token_key 删除令牌
    let delete_result = token::Entity::delete_many()
        .filter(token::Column::TokenKey.eq(&token_key))
        .exec(db)
        .await?;

    Ok(delete_result)
}

// 根据用户名删除令牌的方法
//
// # 参数
// * `username` - 要删除的令牌的用户名
//
// # 返回值
// 返回删除结果，包含删除的行数，或数据库错误
pub async fn delete_token_by_username(username: String) -> Result<DeleteResult, sea_orm::DbErr> {
    let Some(db) = DB.get() else {
            return Err(sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal(
                "Database not initialized".to_owned(),
            )));
        };

    // 根据用户名删除令牌
    let delete_result = token::Entity::delete_many()
        .filter(token::Column::Username.eq(&username))
        .exec(db)
        .await?;

    Ok(delete_result)
}

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
    hasher.update(format!("NODEGET{need_hash}").as_bytes());
    hex::encode(hasher.finalize())
}

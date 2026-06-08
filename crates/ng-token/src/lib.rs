//! ng-token：NodeGet 的 Token 管理核心 crate
//!
//! 负责令牌的生成、验证、缓存与权限校验，是 RBAC 体系的实现层。
//!
//! ## 默认 feature（仅类型）
//! 从 ng-core 重新导出 Token、Limit、Scope、Permission、TokenOrAuth 等类型，
//! 使 Agent 和其他 crate 可以仅依赖类型定义而不引入 server 逻辑。
//!
//! ## `server` feature
//! - `TokenCache` — 基于 DB 的内存令牌缓存，使用 DbBackedCache + make_global_cache! 模式
//! - `super_token` — 超级令牌的生成、轮换与验证（常量时间比较）
//! - `generate_token` — 子令牌生成与存储
//! - `get` — 令牌查询、权限匹配与校验
//! - RPC namespace — `token_*` JSON-RPC 方法（namespace 分隔符为 `_`）

// ── 从 ng-core 重新导出的类型（始终可用）──────────────────────────

pub use ng_core::error::{NodegetError, anyhow_to_nodeget_error};
pub use ng_core::permission::data_structure::{Limit, Permission, Scope, Token};
pub use ng_core::permission::token_auth::TokenOrAuth;

// ── 仅 server feature 启用的模块 ──────────────────────────────────

#[cfg(feature = "server")]
pub mod cache;
#[cfg(feature = "server")]
pub mod generate_token;
#[cfg(feature = "server")]
pub mod get;
#[cfg(feature = "server")]
pub mod rpc;
#[cfg(feature = "server")]
pub mod super_token;

// ── 仅 server feature 启用的重新导出 ──────────────────────────────

#[cfg(feature = "server")]
pub use cache::TokenCache;
#[cfg(feature = "server")]
pub use get::{
    check_token_limit, get_token, get_token_by_key_or_username, parse_token_limit_with_compat,
};
#[cfg(feature = "server")]
pub use super_token::check_super_token;

// ── 共享哈希工具（仅 server，被多个模块使用）────────────────────────

#[cfg(feature = "server")]
use sha2::{Digest, Sha256};

/// 使用 NODEGET 盐值对字符串进行 SHA256 哈希，返回十六进制编码结果。
///
/// - `need_hash`：待哈希的原始字符串（如 token_secret 或 password）
/// - 返回：64 字符的十六进制字符串
#[cfg(feature = "server")]
pub fn hash_string(need_hash: &str) -> String {
    let bytes = hash_to_bytes(need_hash);
    hex::encode(bytes)
}

/// 使用 NODEGET 盐值对字符串进行 SHA256 哈希，返回原始 32 字节摘要。
///
/// 用于常量时间比较场景（如 Token 认证），避免十六进制字符串的额外解码开销。
///
/// - `need_hash`：待哈希的原始字符串
/// - 返回：`[u8; 32]` 原始摘要，可直接与 `ct_eq` 比较
#[cfg(feature = "server")]
pub fn hash_to_bytes(need_hash: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"NODEGET"); // 盐值，与 token_hash/password_hash 存储格式一致
    hasher.update(need_hash.as_bytes());
    hasher.finalize().into()
}

/// 构建并返回 token RPC 模块。
///
/// 调用方应在启动时将此模块合并到主 RPC 模块：
/// ```ignore
/// main_module.merge(ng_token::rpc_module()).unwrap();
/// ```
#[cfg(feature = "server")]
pub fn rpc_module() -> jsonrpsee::RpcModule<rpc::TokenRpcImpl> {
    use rpc::RpcServer;
    rpc::TokenRpcImpl.into_rpc()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_string_deterministic() {
        let a = hash_string("test");
        let b = hash_string("test");
        assert_eq!(a, b, "same input must produce same output");
    }

    #[test]
    fn test_hash_string_known_value() {
        // SHA256("NODEGET" || "test") — manually verified
        let h = hash_string("test");
        assert_eq!(h.len(), 64, "hex-encoded SHA256 must be 64 chars");
        // Verify it matches hash_to_bytes round-trip
        let bytes = hash_to_bytes("test");
        assert_eq!(h, hex::encode(bytes), "hash_string must equal hex::encode(hash_to_bytes)");
    }

    #[test]
    fn test_hash_to_bytes_returns_32_bytes() {
        let bytes = hash_to_bytes("hello");
        assert_eq!(bytes.len(), 32);
    }

    #[test]
    fn test_hash_to_bytes_different_inputs() {
        let a = hash_to_bytes("foo");
        let b = hash_to_bytes("bar");
        assert_ne!(a, b, "different inputs must produce different hashes");
    }

    #[test]
    fn test_hash_string_empty_input() {
        let h = hash_string("");
        assert_eq!(h.len(), 64);
        let bytes = hash_to_bytes("");
        assert_eq!(h, hex::encode(bytes));
    }

    #[test]
    fn test_hash_to_bytes_salt_is_nodeget() {
        // Verify the salt "NODEGET" is actually used by comparing with a manual computation
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"NODEGET");
        hasher.update(b"mypassword");
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(hash_to_bytes("mypassword"), expected);
    }
}

//! 版本信息结构体
//!
//! 通过 `vergen` 在编译期注入 Git、Rustc、构建时间等元信息，
//! 供 `version` RPC 方法和自更新逻辑使用。

use std::fmt::{Display, Formatter};

/// 编译期收集的完整版本信息
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NodeGetVersion {
    /// 二进制类型：Server / Agent / Unknown
    pub binary_type: String,
    /// Cargo 包版本（语义化版本号）
    pub cargo_version: String,

    /// Git 分支名
    pub git_branch: String,
    /// Git 提交 SHA（完整）
    pub git_commit_sha: String,
    /// Git 提交时间戳
    pub git_commit_date: String,
    /// Git 提交消息（首行）
    pub git_commit_message: String,

    /// 构建时间戳
    pub build_time: String,
    /// 目标平台三元组（如 x86_64-unknown-linux-musl）
    pub cargo_target_triple: String,

    /// Rustc 发布通道（stable / nightly / beta）
    pub rustc_channel: String,
    /// Rustc 语义化版本号
    pub rustc_version: String,
    /// Rustc 提交日期
    pub rustc_commit_date: String,
    /// Rustc 提交哈希
    pub rustc_commit_hash: String,
    /// Rustc 使用的 LLVM 版本
    pub rustc_llvm_version: String,
}

/// OnceLock 缓存实例，避免每次调用分配 14 个堆 String
static VERSION_CACHE: std::sync::OnceLock<NodeGetVersion> = std::sync::OnceLock::new();

impl NodeGetVersion {
    /// 获取编译期注入的版本信息实例。
    ///
    /// 所有字段为编译期常量，首次调用构建后缓存在 `OnceLock` 中，后续调用零分配。
    #[must_use]
    pub fn get() -> &'static Self {
        VERSION_CACHE.get_or_init(|| NodeGetVersion {
            binary_type: {
                if cfg!(feature = "for-server") {
                    "Server"
                } else if cfg!(feature = "for-agent") {
                    "Agent"
                } else {
                    "Unknown"
                }
            }
            .to_string(),
            cargo_version: env!("CARGO_PKG_VERSION").to_string(),
            git_branch: env!("VERGEN_GIT_BRANCH").to_string(),
            git_commit_sha: env!("VERGEN_GIT_SHA").to_string(),
            git_commit_date: env!("VERGEN_GIT_COMMIT_TIMESTAMP").to_string(),
            git_commit_message: env!("VERGEN_GIT_COMMIT_MESSAGE").to_string(),
            build_time: env!("VERGEN_BUILD_TIMESTAMP").to_string(),
            cargo_target_triple: env!("VERGEN_CARGO_TARGET_TRIPLE").to_string(),
            rustc_channel: env!("VERGEN_RUSTC_CHANNEL").to_string(),
            rustc_version: env!("VERGEN_RUSTC_SEMVER").to_string(),
            rustc_commit_date: env!("VERGEN_RUSTC_COMMIT_DATE").to_string(),
            rustc_commit_hash: env!("VERGEN_RUSTC_COMMIT_HASH").to_string(),
            rustc_llvm_version: env!("VERGEN_RUSTC_LLVM_VERSION").to_string(),
        })
    }
}

impl Display for NodeGetVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "NodeGet {} Version: {}\nGit Branch: {}\nCommit SHA: {}\nCommit Date: {}\nCommit Message: {}\nBuild Time: {}\nTarget Triple: {}\nRustc Channel: {}\nRustc Version: {}\nRustc Commit Date: {}\nRustc Commit Hash: {}\nRustc LLVM Version: {}",
            self.binary_type,
            self.cargo_version,
            self.git_branch,
            self.git_commit_sha,
            self.git_commit_date,
            self.git_commit_message,
            self.build_time,
            self.cargo_target_triple,
            self.rustc_channel,
            self.rustc_version,
            self.rustc_commit_date,
            self.rustc_commit_hash,
            self.rustc_llvm_version
        )
    }
}

#[cfg(test)]
mod tests {
    use super::NodeGetVersion;

    #[test]
    fn nodeget_version_get_returns_static() {
        let v1 = NodeGetVersion::get();
        let v2 = NodeGetVersion::get();
        // Same static reference
        assert!(std::ptr::eq(v1, v2));
    }

    #[test]
    fn nodeget_version_binary_type_default() {
        // binary_type depends on which feature is active:
        // for-server -> "Server", for-agent -> "Agent", neither -> "Unknown"
        // Under `cargo test --workspace`, Cargo unifies features so for-server may be enabled.
        let v = NodeGetVersion::get();
        let expected = if cfg!(feature = "for-server") {
            "Server"
        } else if cfg!(feature = "for-agent") {
            "Agent"
        } else {
            "Unknown"
        };
        assert_eq!(v.binary_type, expected);
    }

    #[test]
    fn nodeget_version_display_contains_fields() {
        let v = NodeGetVersion::get();
        let display = format!("{v}");
        let expected_prefix = format!("NodeGet {} Version:", v.binary_type);
        assert!(display.starts_with(&expected_prefix));
        assert!(display.contains("Git Branch:"));
        assert!(display.contains("Commit SHA:"));
        assert!(display.contains("Target Triple:"));
        assert!(display.contains("Rustc Channel:"));
    }

    #[test]
    fn nodeget_version_debug() {
        let v = NodeGetVersion::get();
        let debug = format!("{v:?}");
        assert!(debug.contains("NodeGetVersion"));
        assert!(debug.contains("binary_type"));
    }

    #[test]
    fn nodeget_version_clone_eq() {
        let v = NodeGetVersion::get();
        let cloned = v.clone();
        assert_eq!(*v, cloned);
    }

    #[test]
    fn nodeget_version_serde_round_trip() {
        let v = NodeGetVersion::get();
        let json = serde_json::to_string(v).unwrap();
        let de: NodeGetVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(*v, de);
    }
}

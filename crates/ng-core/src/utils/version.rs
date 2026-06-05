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

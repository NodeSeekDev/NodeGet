// NodeGet 版本信息结构体，包含构建时的版本和环境信息
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct NodeGetVersion {
    // 二进制类型，"Server" 或 "Agent"
    pub binary_type: String, // "Server" / "Agent"
    // Cargo 包版本
    pub cargo_version: String, // CARGO_PKG_VERSION

    // Git 分支名
    pub git_branch: String, // VERGEN_GIT_BRANCH
    // Git 提交 SHA
    pub git_commit_sha: String, // VERGEN_GIT_SHA
    // Git 提交日期时间戳
    pub git_commit_date: String, // VERGEN_GIT_COMMIT_TIMESTAMP
    // Git 提交消息
    pub git_commit_message: String, // VERGEN_GIT_COMMIT_MESSAGE

    // 构建时间戳
    pub build_time: String, // VERGEN_BUILD_TIMESTAMP
    // Cargo 目标三元组
    pub cargo_target_triple: String, // VERGEN_CARGO_TARGET_TRIPLE

    // Rust 编译器通道
    pub rustc_channel: String, // VERGEN_RUSTC_CHANNEL
    // Rust 编译器版本
    pub rustc_version: String, // VERGEN_RUSTC_SEMVER
    // Rust 编译器提交日期
    pub rustc_commit_date: String, // VERGEN_RUSTC_COMMIT_DATE
    // Rust 编译器提交哈希
    pub rustc_commit_hash: String, // VERGEN_RUSTC_COMMIT_HASH
    // Rust 编译器 LLVM 版本
    pub rustc_llvm_version: String, // VERGEN_RUSTC_LLVM_VERSION
}

impl NodeGetVersion {
    // 获取当前构建的版本信息
    //
    // # 返回值
    // 返回包含当前构建版本信息的 NodeGetVersion 实例
    #[must_use]
    pub fn get() -> Self {
        Self {
            binary_type: {
                if cfg!(feature = "for-server") {
                    "Server".to_string()
                } else if cfg!(feature = "for-agent") {
                    "Agent".to_string()
                } else {
                    "Unknown".to_string()
                }
            },
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
        }
    }
}

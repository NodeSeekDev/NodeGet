#[derive(Debug, Clone)]
#[cfg_attr(feature = "for-server", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(
    feature = "for-agent",
    derive(miniserde::Deserialize, miniserde::Serialize)
)]
pub struct NodeGetVersion {
    pub binary_type: String,   // "Server" / "Agent"
    pub cargo_version: String, // CARGO_PKG_VERSION

    pub git_branch: String,         // VERGEN_GIT_BRANCH
    pub git_commit_sha: String,     // VERGEN_GIT_SHA
    pub git_commit_date: String,    // VERGEN_GIT_COMMIT_TIMESTAMP
    pub git_commit_message: String, // VERGEN_GIT_COMMIT_MESSAGE

    pub build_time: String,          // VERGEN_BUILD_TIMESTAMP
    pub cargo_target_triple: String, // VERGEN_CARGO_TARGET_TRIPLE

    pub rustc_channel: String,      // VERGEN_RUSTC_CHANNEL
    pub rustc_version: String,      // VERGEN_RUSTC_SEMVER
    pub rustc_commit_date: String,  // VERGEN_RUSTC_COMMIT_DATE
    pub rustc_commit_hash: String,  // VERGEN_RUSTC_COMMIT_HASH
    pub rustc_llvm_version: String, // VERGEN_RUSTC_LLVM_VERSION
}

impl NodeGetVersion {
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

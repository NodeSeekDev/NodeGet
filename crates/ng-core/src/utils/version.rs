use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct NodeGetVersion {
    pub binary_type: &'static str,
    pub cargo_version: &'static str,

    pub git_branch: &'static str,
    pub git_commit_sha: &'static str,
    pub git_commit_date: &'static str,
    pub git_commit_message: &'static str,

    pub build_time: &'static str,
    pub cargo_target_triple: &'static str,

    pub rustc_channel: &'static str,
    pub rustc_version: &'static str,
    pub rustc_commit_date: &'static str,
    pub rustc_commit_hash: &'static str,
    pub rustc_llvm_version: &'static str,
}

impl NodeGetVersion {
    #[must_use]
    pub fn get() -> Self {
        Self {
            binary_type: {
                if cfg!(feature = "for-server") {
                    "Server"
                } else if cfg!(feature = "for-agent") {
                    "Agent"
                } else {
                    "Unknown"
                }
            },
            cargo_version: env!("CARGO_PKG_VERSION"),
            git_branch: env!("VERGEN_GIT_BRANCH"),
            git_commit_sha: env!("VERGEN_GIT_SHA"),
            git_commit_date: env!("VERGEN_GIT_COMMIT_TIMESTAMP"),
            git_commit_message: env!("VERGEN_GIT_COMMIT_MESSAGE"),
            build_time: env!("VERGEN_BUILD_TIMESTAMP"),
            cargo_target_triple: env!("VERGEN_CARGO_TARGET_TRIPLE"),
            rustc_channel: env!("VERGEN_RUSTC_CHANNEL"),
            rustc_version: env!("VERGEN_RUSTC_SEMVER"),
            rustc_commit_date: env!("VERGEN_RUSTC_COMMIT_DATE"),
            rustc_commit_hash: env!("VERGEN_RUSTC_COMMIT_HASH"),
            rustc_llvm_version: env!("VERGEN_RUSTC_LLVM_VERSION"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for NodeGetVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Helper {
            binary_type: String,
            cargo_version: String,
            git_branch: String,
            git_commit_sha: String,
            git_commit_date: String,
            git_commit_message: String,
            build_time: String,
            cargo_target_triple: String,
            rustc_channel: String,
            rustc_version: String,
            rustc_commit_date: String,
            rustc_commit_hash: String,
            rustc_llvm_version: String,
        }

        let h = Helper::deserialize(deserializer)?;
        Ok(Self {
            binary_type: Box::leak(h.binary_type.into_boxed_str()),
            cargo_version: Box::leak(h.cargo_version.into_boxed_str()),
            git_branch: Box::leak(h.git_branch.into_boxed_str()),
            git_commit_sha: Box::leak(h.git_commit_sha.into_boxed_str()),
            git_commit_date: Box::leak(h.git_commit_date.into_boxed_str()),
            git_commit_message: Box::leak(h.git_commit_message.into_boxed_str()),
            build_time: Box::leak(h.build_time.into_boxed_str()),
            cargo_target_triple: Box::leak(h.cargo_target_triple.into_boxed_str()),
            rustc_channel: Box::leak(h.rustc_channel.into_boxed_str()),
            rustc_version: Box::leak(h.rustc_version.into_boxed_str()),
            rustc_commit_date: Box::leak(h.rustc_commit_date.into_boxed_str()),
            rustc_commit_hash: Box::leak(h.rustc_commit_hash.into_boxed_str()),
            rustc_llvm_version: Box::leak(h.rustc_llvm_version.into_boxed_str()),
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

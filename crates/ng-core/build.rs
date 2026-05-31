use std::process::Command;
use std::time::SystemTime;

const UNKNOWN: &str = "UNKNOWN";

fn run(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| UNKNOWN.to_string())
}

fn main() {
    // Git info
    let branch = run("git", &["rev-parse", "--abbrev-ref", "HEAD"]);
    let sha = run("git", &["rev-parse", "HEAD"]);
    let commit_ts = run("git", &["log", "-1", "--format=%cI"]);
    let commit_msg = run("git", &["log", "-1", "--format=%s"]);

    // Build timestamp (seconds since UNIX epoch)
    let build_time = {
        let d = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{d}")
    };

    // Cargo target triple
    let target_triple = std::env::var("TARGET").unwrap_or_else(|_| UNKNOWN.to_string());

    // Rustc info
    let rustc_verbose = run("rustc", &["-vV"]);
    let mut rustc_semver = UNKNOWN.to_string();
    let mut rustc_channel = UNKNOWN.to_string();
    let mut rustc_commit_date = UNKNOWN.to_string();
    let mut rustc_commit_hash = UNKNOWN.to_string();
    let mut rustc_llvm = UNKNOWN.to_string();
    for line in rustc_verbose.lines() {
        if let Some(v) = line.strip_prefix("release: ") {
            rustc_semver = v.to_string();
            rustc_channel = if v.contains("nightly") {
                "nightly"
            } else if v.contains("beta") {
                "beta"
            } else {
                "stable"
            }
            .to_string();
        } else if let Some(v) = line.strip_prefix("commit-date: ") {
            rustc_commit_date = v.to_string();
        } else if let Some(v) = line.strip_prefix("commit-hash: ") {
            rustc_commit_hash = v.to_string();
        } else if let Some(v) = line.strip_prefix("LLVM version: ") {
            rustc_llvm = v.to_string();
        }
    }

    // Emit cargo instructions — same env var names as vergen
    println!("cargo:rustc-env=VERGEN_GIT_BRANCH={branch}");
    println!("cargo:rustc-env=VERGEN_GIT_SHA={sha}");
    println!("cargo:rustc-env=VERGEN_GIT_COMMIT_TIMESTAMP={commit_ts}");
    println!("cargo:rustc-env=VERGEN_GIT_COMMIT_MESSAGE={commit_msg}");
    println!("cargo:rustc-env=VERGEN_BUILD_TIMESTAMP={build_time}");
    println!("cargo:rustc-env=VERGEN_CARGO_TARGET_TRIPLE={target_triple}");
    println!("cargo:rustc-env=VERGEN_RUSTC_CHANNEL={rustc_channel}");
    println!("cargo:rustc-env=VERGEN_RUSTC_SEMVER={rustc_semver}");
    println!("cargo:rustc-env=VERGEN_RUSTC_COMMIT_DATE={rustc_commit_date}");
    println!("cargo:rustc-env=VERGEN_RUSTC_COMMIT_HASH={rustc_commit_hash}");
    println!("cargo:rustc-env=VERGEN_RUSTC_LLVM_VERSION={rustc_llvm}");

    // Re-run if git HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
}

use nodeget_lib::utils::version::NodeGetVersion;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use log::{error, info};

const ARCH_NAME: [(&str, &str); 24] = [
    // Linux x86_64
    (
        "x86_64-unknown-linux-musl",
        "nodeget-agent-linux-x86_64-musl",
    ),
    ("x86_64-unknown-linux-gnu", "nodeget-agent-linux-x86_64-gnu"),
    // Linux i686
    ("i686-unknown-linux-gnu", "nodeget-agent-linux-i686-gnu"),
    ("i686-unknown-linux-musl", "nodeget-agent-linux-i686-musl"),
    // Linux aarch64
    (
        "aarch64-unknown-linux-gnu",
        "nodeget-agent-linux-aarch64-gnu",
    ),
    (
        "aarch64-unknown-linux-musl",
        "nodeget-agent-linux-aarch64-musl",
    ),
    // Linux arm
    (
        "arm-unknown-linux-gnueabi",
        "nodeget-agent-linux-arm-gnueabi",
    ),
    (
        "arm-unknown-linux-gnueabihf",
        "nodeget-agent-linux-arm-gnueabihf",
    ),
    (
        "arm-unknown-linux-musleabi",
        "nodeget-agent-linux-arm-musleabi",
    ),
    (
        "arm-unknown-linux-musleabihf",
        "nodeget-agent-linux-arm-musleabihf",
    ),
    // Linux armv7
    (
        "armv7-unknown-linux-gnueabi",
        "nodeget-agent-linux-armv7-gnueabi",
    ),
    (
        "armv7-unknown-linux-gnueabihf",
        "nodeget-agent-linux-armv7-gnueabihf",
    ),
    (
        "armv7-unknown-linux-musleabi",
        "nodeget-agent-linux-armv7-musleabi",
    ),
    (
        "armv7-unknown-linux-musleabihf",
        "nodeget-agent-linux-armv7-musleabihf",
    ),
    // Linux thumbv7neon
    (
        "thumbv7neon-unknown-linux-gnueabihf",
        "nodeget-agent-linux-thumbv7neon-gnueabihf",
    ),
    // Linux riscv64 / powerpc / s390x / sparc64
    (
        "riscv64gc-unknown-linux-gnu",
        "nodeget-agent-linux-riscv64gc-gnu",
    ),
    (
        "powerpc64-unknown-linux-gnu",
        "nodeget-agent-linux-powerpc64-gnu",
    ),
    (
        "powerpc64le-unknown-linux-gnu",
        "nodeget-agent-linux-powerpc64le-gnu",
    ),
    ("s390x-unknown-linux-gnu", "nodeget-agent-linux-s390x-gnu"),
    (
        "sparc64-unknown-linux-gnu",
        "nodeget-agent-linux-sparc64-gnu",
    ),
    // Windows
    ("x86_64-pc-windows-msvc", "nodeget-agent-windows-x86_64.exe"),
    ("i686-pc-windows-msvc", "nodeget-agent-windows-i686.exe"),
    (
        "aarch64-pc-windows-msvc",
        "nodeget-agent-windows-aarch64.exe",
    ),
    // macOS
    ("aarch64-apple-darwin", "nodeget-agent-macos-aarch64"),
];

fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let body = s.strip_prefix('v')?;
    let mut parts = body.splitn(3, '.');
    let x: u32 = parts.next()?.parse().ok()?;
    let y: u32 = parts.next()?.parse().ok()?;
    let z: u32 = parts.next()?.parse().ok()?;
    Some((x, y, z))
}

fn should_update(target: (u32, u32, u32), current: (u32, u32, u32)) -> bool {
    target.0 > current.0
        || (target.0 == current.0 && target.1 > current.1)
        || (target.0 == current.0 && target.1 == current.1 && target.2 >= current.2)
}

pub async fn self_update(tag: &str) -> bool {
    let target_version = match parse_version(tag) {
        None => {
            log::error!("Invalid version tag: {tag}");
            return false;
        }
        Some(v) => v,
    };

    let current_version =
        parse_version(&format!("v{}", NodeGetVersion::get().cargo_version)).unwrap();

    let should_update = should_update(target_version, current_version);
    if !should_update {
        return false;
    }

    log::info!("Updating from version {}.{}.{} to {}.{}.{}",
        current_version.0, current_version.1, current_version.2,
        target_version.0, target_version.1, target_version.2
    );

    let arch_str = NodeGetVersion::get().cargo_target_triple;

    let (_, binary_name) = match ARCH_NAME.iter().find(|(target, _)| *target == arch_str) {
        Some(pair) => pair,
        None => {
            log::warn!("Current architecture {arch_str} is not supported for self-update");
            return false;
        }
    };

    let url = format!(
        "https://install.nodeget.com/releases/{}?tag={}",
        binary_name, tag
    );

    log::info!("Downloading update from {url}");

    let client = reqwest::Client::new();
    let response = match client
        .get(&url)
        .header("User-Agent", "curl/8.7.1")
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("Download request failed: {e}");
            return false;
        }
    };

    if !response.status().is_success() {
        log::error!("Download failed with status: {}", response.status());
        return false;
    }

    let bytes
        = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            log::error!("Failed to read response body: {e}");
            return false;
        }
    };

    if bytes.len() < 1024 {
        log::error!("Downloaded file too small ({} bytes), aborting", bytes.len());
        return false;
    }

    log::info!("Downloaded {} bytes ", bytes.len());

    let current = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log::error!("Failed to get current exe path: {e}");
            return false;
        }
    };

    let mut backup = current.as_os_str().to_os_string();
    backup.push(".old");

    if let Err(e) = std::fs::rename(&current, &backup) {
        log::error!("Failed to backup binary: {e}");
        return false;
    }

    if let Err(e) = std::fs::write(&current, &bytes) {
        log::error!("Failed to replace binary: {e}");
        // Try to restore backup
        let _ = std::fs::rename(&backup, &current);
        log::info!("Restored original binary from backup");
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        if let Err(e) = std::fs::set_permissions(&current, perms) {
            log::warn!("Failed to set executable permission: {e}");
        }
    }

    log::info!("Binary replaced successfully: {}", current.display());
    true
}

/// Restart the current process by replacing the running binary.
/// This function never returns on success.
pub fn restart_process() -> ! {
    let current = std::env::current_exe().unwrap_or_else(|e| {
        log::error!("Failed to get current exe path: {e}");
        std::process::exit(1);
    });

    let mut args = std::env::args();
    let _ = args.next(); // skip program name

    log::info!("Restarting agent: {}", current.display());

    let err = std::process::Command::new(&current)
        .args(args)
        .exec();

    log::error!("Failed to restart: {err}");
    std::process::exit(1);
}

#[cfg(unix)]
pub fn restart_process_with_exec_v() -> ! {
    use std::ffi::CString;
    use std::os::raw::c_char;
    use std::ptr;

    let current = std::env::current_exe().unwrap_or_else(|e| {
        log::error!("Failed to get current exe path: {e}");
        std::process::exit(1);
    });

    let path = CString::new(current.to_str().unwrap()).unwrap();

    // 每个参数转成独立的 CString，Vec 保活指针
    let c_args: Vec<CString> = std::env::args()
        .map(|s| CString::new(s).unwrap())
        .collect();

    let mut ptrs: Vec<*const c_char> = c_args.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(ptr::null()); // argv 以 NULL 结尾

    info!("Starting execv...");

    unsafe {
        libc::execv(path.as_ptr(), ptrs.as_ptr());

        // execv 只在失败时返回
        error!("execv failed: {}", std::io::Error::last_os_error());
        std::process::exit(1);
    }
}
use crate::utils::version::NodeGetVersion;

#[cfg(feature = "for-agent")]
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

#[cfg(feature = "for-server")]
const SERVER_ARCH_NAME: [(&str, &str); 10] = [
    // Linux x86_64
    (
        "x86_64-unknown-linux-musl",
        "nodeget-server-linux-x86_64-musl",
    ),
    ("x86_64-unknown-linux-gnu", "nodeget-server-linux-x86_64-gnu"),
    // Linux aarch64
    (
        "aarch64-unknown-linux-gnu",
        "nodeget-server-linux-aarch64-gnu",
    ),
    (
        "aarch64-unknown-linux-musl",
        "nodeget-server-linux-aarch64-musl",
    ),
    // Linux armv7
    (
        "armv7-unknown-linux-gnueabi",
        "nodeget-server-linux-armv7-gnueabi",
    ),
    (
        "armv7-unknown-linux-gnueabihf",
        "nodeget-server-linux-armv7-gnueabihf",
    ),
    (
        "armv7-unknown-linux-musleabi",
        "nodeget-server-linux-armv7-musleabi",
    ),
    (
        "armv7-unknown-linux-musleabihf",
        "nodeget-server-linux-armv7-musleabihf",
    ),
    // Windows
    ("x86_64-pc-windows-msvc", "nodeget-server-windows-x86_64.exe"),
    // macOS
    ("aarch64-apple-darwin", "nodeget-server-macos-aarch64"),
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
    true
}

pub fn check_if_update_needed(tag: &str) -> ((u32, u32, u32),(u32, u32, u32), bool) {
    let target_version = match parse_version(tag) {
        None => {
            return ((0, 0, 0), (0, 0, 0), false);
        }
        Some(v) => v,
    };

    let current_version =
        match parse_version(&format!("v{}", NodeGetVersion::get().cargo_version)) {
            None => {
                return ((0, 0, 0), target_version, false);
            }
            Some(v) => v,
        };

    (current_version,target_version, should_update(target_version, current_version))
}

#[cfg(feature = "for-agent")]
pub fn get_url(tag: &str) -> Option<String> {
    let arch_str = NodeGetVersion::get().cargo_target_triple;

    let (_, binary_name) = match ARCH_NAME.iter().find(|(target, _)| *target == arch_str) {
        Some(pair) => pair,
        None => {
            return None;
        }
    };

    let url = format!(
        "https://install.nodeget.com/releases/{}?tag={}",
        binary_name, tag
    );

    Some(url)
}

#[cfg(feature = "for-server")]
pub fn get_server_url(tag: &str) -> Option<String> {
    let arch_str = NodeGetVersion::get().cargo_target_triple;

    let (_, binary_name) = match SERVER_ARCH_NAME.iter().find(|(target, _)| *target == arch_str) {
        Some(pair) => pair,
        None => {
            return None;
        }
    };

    let url = format!(
        "https://install.nodeget.com/releases/{}?tag={}",
        binary_name, tag
    );

    Some(url)
}

pub fn replace_binary(binary: Vec<u8>) -> bool {
    let current = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => {
            return false;
        }
    };

    let mut backup = current.as_os_str().to_os_string();
    backup.push(".old");

    if std::fs::rename(&current, &backup).is_err() {
        return false;
    }

    if std::fs::write(&current, &binary).is_err() {
        // Try to restore backup
        let _ = std::fs::rename(&backup, &current);
        return false;
    }

    true
}


#[cfg(not(unix))]
pub fn restart_process() -> ! {
    let current = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("Failed to get current exe path: {e}");
        std::process::exit(1);
    });

    let mut args = std::env::args();
    let _ = args.next(); // skip program name

    println!("Restarting agent: {}", current.display());

    match std::process::Command::new(&current).args(args).spawn() {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            eprintln!("Failed to restart: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(unix)]
pub fn restart_process_with_exec_v() -> ! {
    use std::ffi::CString;
    use std::os::raw::c_char;
    use std::ptr;

    let current = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("Failed to get current exe path: {e}");
        std::process::exit(1);
    });

    let path = CString::new(current.to_str().unwrap()).unwrap();

    // 每个参数转成独立的 CString，Vec 保活指针
    let c_args: Vec<CString> = std::env::args()
        .map(|s| CString::new(s).unwrap())
        .collect();

    let mut ptrs: Vec<*const c_char> = c_args.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(ptr::null()); // argv 以 NULL 结尾

    println!("Starting execv...");

    unsafe {
        libc::execv(path.as_ptr(), ptrs.as_ptr());

        // execv 只在失败时返回
        eprintln!("execv failed: {}", std::io::Error::last_os_error());
        std::process::exit(1);
    }
}
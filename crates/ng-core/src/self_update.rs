//! 自更新逻辑
//!
//! 提供 Server / Agent 的在线升级能力：版本比较、下载 URL 构造、
//! 二进制替换与进程重启。Agent 与 Server 通过 feature gate 分别
//! 编译各自的架构映射表和重启策略。

use crate::utils::version::NodeGetVersion;

/// Agent 架构到发布文件名的映射表。
/// 元组格式：(Cargo target triple, 发布文件名)
#[cfg(feature = "for-agent")]
const ARCH_NAME: [(&str, &str); 24] = [
    (
        "x86_64-unknown-linux-musl",
        "nodeget-agent-linux-x86_64-musl",
    ),
    ("x86_64-unknown-linux-gnu", "nodeget-agent-linux-x86_64-gnu"),
    ("i686-unknown-linux-gnu", "nodeget-agent-linux-i686-gnu"),
    ("i686-unknown-linux-musl", "nodeget-agent-linux-i686-musl"),
    (
        "aarch64-unknown-linux-gnu",
        "nodeget-agent-linux-aarch64-gnu",
    ),
    (
        "aarch64-unknown-linux-musl",
        "nodeget-agent-linux-aarch64-musl",
    ),
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
    (
        "thumbv7neon-unknown-linux-gnueabihf",
        "nodeget-agent-linux-thumbv7neon-gnueabihf",
    ),
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
    ("x86_64-pc-windows-msvc", "nodeget-agent-windows-x86_64.exe"),
    ("i686-pc-windows-msvc", "nodeget-agent-windows-i686.exe"),
    (
        "aarch64-pc-windows-msvc",
        "nodeget-agent-windows-aarch64.exe",
    ),
    ("aarch64-apple-darwin", "nodeget-agent-macos-aarch64"),
];

/// Server 架构到发布文件名的映射表。
#[cfg(feature = "for-server")]
const SERVER_ARCH_NAME: [(&str, &str); 10] = [
    (
        "x86_64-unknown-linux-musl",
        "nodeget-server-linux-x86_64-musl",
    ),
    (
        "x86_64-unknown-linux-gnu",
        "nodeget-server-linux-x86_64-gnu",
    ),
    (
        "aarch64-unknown-linux-gnu",
        "nodeget-server-linux-aarch64-gnu",
    ),
    (
        "aarch64-unknown-linux-musl",
        "nodeget-server-linux-aarch64-musl",
    ),
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
    (
        "x86_64-pc-windows-msvc",
        "nodeget-server-windows-x86_64.exe",
    ),
    ("aarch64-apple-darwin", "nodeget-server-macos-aarch64"),
];

/// 解析 `vX.Y.Z` 格式的版本字符串为三元组。
///
/// - `s`：以 `v` 开头的版本字符串
/// - 返回 (major, minor, patch) 或 None
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let body = s.strip_prefix('v')?;
    parse_version_body(body)
}

fn parse_version_body(body: impl AsRef<str>) -> Option<(u32, u32, u32)> {
    let mut parts = body.as_ref().splitn(3, '.');
    let x: u32 = parts.next()?.parse().ok()?;
    let y: u32 = parts.next()?.parse().ok()?;
    let z: u32 = parts.next()?.parse().ok()?;
    Some((x, y, z))
}

/// 判断是否需要更新：目标版本与当前版本不同时返回 true。
fn should_update(target: (u32, u32, u32), current: (u32, u32, u32)) -> bool {
    target != current
}

/// 获取当前进程对应的原始二进制路径。
///
/// 如果 `current_exe()` 因之前的 self_update rename 而指向 `.old` / `.old.old` …
/// 则把所有末尾的 `.old` 扩展名剥掉，确保始终指向用户真正启动的那个文件。
///
/// - 返回规范化后的路径，获取失败返回 None
pub fn canonical_exe_path() -> Option<std::path::PathBuf> {
    let mut path = std::env::current_exe().ok()?;
    while path.extension() == Some(std::ffi::OsStr::new("old")) {
        path = path.with_extension("");
    }
    Some(path)
}

/// 检查是否需要更新，比较目标版本与当前版本。
///
/// - `tag`：目标版本标签（如 `v0.5.2`）
/// - 返回 (当前版本, 目标版本, 是否需要更新)
///   版本解析失败时对应版本为 (0,0,0) 且不需要更新
pub fn check_if_update_needed(tag: &str) -> ((u32, u32, u32), (u32, u32, u32), bool) {
    let target_version = match parse_version(tag) {
        None => {
            return ((0, 0, 0), (0, 0, 0), false);
        }
        Some(v) => v,
    };

    let current_version = match parse_version_body(NodeGetVersion::get().cargo_version.clone())
    {
        None => {
            return ((0, 0, 0), target_version, false);
        }
        Some(v) => v,
    };

    (
        current_version,
        target_version,
        should_update(target_version, current_version),
    )
}

/// 根据架构映射表构建下载 URL。
///
/// 1. 读取编译期注入的 target triple
/// 2. 在映射表中查找对应的发布文件名
/// 3. 拼接为 `https://install.nodeget.com/releases/{name}?tag={tag}`
///
/// - `arch_name`：架构映射表
/// - `tag`：版本标签
/// - 返回下载 URL，未找到对应架构时返回 None
#[cfg(any(feature = "for-agent", feature = "for-server"))]
fn build_release_url(arch_name: &[(&str, &str)], tag: &str) -> Option<String> {
    let arch_str = &NodeGetVersion::get().cargo_target_triple;

    let (_, binary_name) = match arch_name.iter().find(|(target, _)| *target == arch_str) {
        Some(pair) => pair,
        None => {
            return None;
        }
    };

    Some(format!(
        "https://install.nodeget.com/releases/{}?tag={}",
        binary_name, tag
    ))
}

/// 获取 Agent 下载 URL。
///
/// - `tag`：版本标签
/// - 返回 Agent 对应平台的下载 URL
#[cfg(feature = "for-agent")]
pub fn get_url(tag: &str) -> Option<String> {
    build_release_url(&ARCH_NAME, tag)
}

/// 获取 Server 下载 URL。
///
/// - `tag`：版本标签
/// - 返回 Server 对应平台的下载 URL
#[cfg(feature = "for-server")]
pub fn get_server_url(tag: &str) -> Option<String> {
    build_release_url(&SERVER_ARCH_NAME, tag)
}

/// 替换当前二进制文件并保留备份。
///
/// 1. 获取当前可执行文件路径
/// 2. 将当前文件重命名为 `.old` 备份
/// 3. 写入新二进制内容
/// 4. 写入失败时自动回滚（恢复备份）
///
/// - `binary`：新二进制的完整内容
/// - 返回是否替换成功
pub fn replace_binary(binary: Vec<u8>) -> bool {
    let current = match canonical_exe_path() {
        Some(p) => p,
        None => {
            tracing::error!("Failed to get canonical exe path for binary replacement");
            return false;
        }
    };

    let mut backup = current.as_os_str().to_os_string();
    backup.push(".old");

    if std::fs::rename(&current, &backup).is_err() {
        tracing::error!("Failed to rename current binary to backup");
        return false;
    }

    if std::fs::write(&current, &binary).is_err() {
        // 写入失败，尝试回滚
        if let Err(e) = std::fs::rename(&backup, &current) {
            tracing::error!(error = %e, "Failed to restore backup during rollback");
        }
        return false;
    }

    true
}

/// 非 Unix 平台的进程重启：spawn 子进程后退出当前进程。
///
/// - 获取当前可执行文件路径和命令行参数
/// - 启动新进程后当前进程退出
#[cfg(all(not(unix), any(feature = "for-agent", feature = "for-server")))]
pub fn restart_process() -> ! {
    let current = canonical_exe_path().unwrap_or_else(|| {
        tracing::error!("Failed to get canonical exe path");
        std::process::exit(1);
    });

    let mut args = std::env::args();
    let _ = args.next(); // 跳过程序名

    tracing::info!("Restarting agent: {}", current.display());

    match std::process::Command::new(&current).args(args).spawn() {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            tracing::error!("Failed to restart: {e}");
            std::process::exit(1);
        }
    }
}

/// Unix 平台的进程重启：委托给 `restart_process_with_exec_v`。
#[cfg(all(unix, any(feature = "for-agent", feature = "for-server")))]
pub fn restart_process() -> ! {
    restart_process_with_exec_v()
}

/// Unix 平台使用 `execv` 原地替换进程映像，无需 fork。
///
/// 1. 获取当前可执行文件路径
/// 2. 构造 C 字符串参数数组
/// 3. 调用 `libc::execv` 替换当前进程
///
/// execv 仅在失败时返回，成功时当前进程已被新映像取代。
#[cfg(all(unix, any(feature = "for-agent", feature = "for-server")))]
pub fn restart_process_with_exec_v() -> ! {
    use std::ffi::CString;
    use std::os::raw::c_char;
    use std::ptr;

    let current = canonical_exe_path().unwrap_or_else(|| {
        tracing::error!("Failed to get canonical exe path");
        std::process::exit(1);
    });

    let path = CString::new(current.to_str().unwrap()).unwrap();

    let c_args: Vec<CString> = std::env::args().map(|s| CString::new(s).unwrap()).collect();

    let mut ptrs: Vec<*const c_char> = c_args.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(ptr::null()); // C 约定：参数数组以 NULL 结尾

    tracing::info!("Starting execv...");

    // SAFETY: execv 替换当前进程映像。所有指针（`path` 和 `ptrs`）
    // 源自本函数内持有的 CString，execv 仅在失败时返回，
    // 因此不存在悬垂指针问题。
    unsafe {
        libc::execv(path.as_ptr(), ptrs.as_ptr());
    }

    tracing::error!("execv failed: {}", std::io::Error::last_os_error());
    std::process::exit(1);
}

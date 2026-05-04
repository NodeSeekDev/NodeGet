use libc::if_data;
use nodeget_lib::utils::version::NodeGetVersion;
use std::fmt::Display;

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

fn parse_version(s: String) -> Option<(u32, u32, u32)> {
    let body = s.strip_prefix('v')?;
    let mut parts = body.splitn(3, '.');
    let x: u32 = parts.next()?.parse().ok()?;
    let y: u32 = parts.next()?.parse().ok()?;
    let z: u32 = parts.next()?.parse().ok()?;
    Some((x, y, z))
}

pub async fn self_update(tag: &str) -> bool {
    todo!()
}

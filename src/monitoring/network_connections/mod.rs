#[cfg(target_os = "linux")]
mod netlink;

#[cfg(target_os = "linux")]
pub fn calc_connections() -> (u64, u64) {
    // (udp, tcp)
    use netlink::connections_count_with_protocol;
    let tcp4 =
        connections_count_with_protocol(libc::AF_INET as u8, libc::IPPROTO_TCP as u8).unwrap_or(0);
    let tcp6 =
        connections_count_with_protocol(libc::AF_INET6 as u8, libc::IPPROTO_TCP as u8).unwrap_or(0);
    let udp4 =
        connections_count_with_protocol(libc::AF_INET as u8, libc::IPPROTO_UDP as u8).unwrap_or(0);
    let udp6 =
        connections_count_with_protocol(libc::AF_INET6 as u8, libc::IPPROTO_UDP as u8).unwrap_or(0);
    (udp4 + udp6, tcp4 + tcp6)
}

#[cfg(target_os = "windows")]
pub fn calc_connections() -> (u64, u64) {
    use netstat2::{ProtocolFlags, ProtocolSocketInfo, iterate_sockets_info_without_pids};

    iterate_sockets_info_without_pids(ProtocolFlags::TCP | ProtocolFlags::UDP)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .fold((0, 0), |(udp, tcp), info| match info.protocol_socket_info {
            ProtocolSocketInfo::Tcp(_) => (udp, tcp + 1),
            ProtocolSocketInfo::Udp(_) => (udp + 1, tcp),
        })
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn calc_connections() -> (u64, u64) {
    (0, 0) // TODO: MacOS Support
}

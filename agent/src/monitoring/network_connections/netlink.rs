//! Linux Netlink SOCK_DIAG 网络连接统计实现。
//!
//! 通过 Netlink 的 `SOCK_DIAG_BY_FAMILY` 请求查询内核 socket 表，
//! 统计指定协议族和传输协议的连接数。TCP 仅统计 `ESTABLISHED` 状态。
//!
//! 参考：linux/inet_diag.h、linux/sock_diag.h

use libc::{c_void, close, recvfrom, sendto, sockaddr, sockaddr_nl, socket};
use log::{error, warn};
use std::io;
use std::mem::{size_of, zeroed};
use std::os::fd::RawFd;
use std::ptr;

/// Netlink 请求类型：按协议族查询 socket 诊断信息。
const SOCK_DIAG_BY_FAMILY: u16 = 20;
/// 查询所有 TCP 状态的位掩码。
const ALL_TCP_STATES: u32 = 0xffffffff;
/// TCP ESTABLISHED 状态的位索引。
const TCP_ESTABLISHED: u32 = 1;
/// Netlink 消息头部长度。
const NLMSG_HDRLEN: usize = size_of::<libc::nlmsghdr>();

/// ---- 与内核对齐的 C 结构体定义 ----

/// 对应 linux/inet_diag.h 的 `inet_diag_sock_id`。
#[repr(C)]
#[derive(Clone, Copy)]
struct InetDiagSockId {
    /// 源端口
    idiag_sport: u16,
    /// 目标端口
    idiag_dport: u16,
    /// 源地址，长度足以容纳 IPv6（IPv4 仅使用 `[0]`）
    idiag_src: [u32; 4],
    /// 目标地址
    idiag_dst: [u32; 4],
    /// 接口索引
    idiag_if: u32,
    /// 内核 Cookie（用于唯一标识 socket）
    idiag_cookie: [u32; 2],
}

/// 对应 linux/inet_diag.h 的 `inet_diag_req_v2`。
#[repr(C)]
#[derive(Clone, Copy)]
struct InetDiagReqV2 {
    /// 协议族（AF_INET / AF_INET6）
    family: u8,
    /// 传输协议（IPPROTO_TCP / IPPROTO_UDP）
    protocol: u8,
    /// 扩展信息位掩码
    ext: u8,
    /// 对齐填充
    pad: u8,
    /// 查询的状态位掩码
    states: u32,
    /// Socket 标识查询条件
    id: InetDiagSockId,
}

/// 按协议族和传输协议查询连接数。
///
/// - `family` - 协议族（`AF_INET` 或 `AF_INET6`）
/// - `protocol` - 传输协议（`IPPROTO_TCP` 或 `IPPROTO_UDP`）
///
/// TCP 仅查询 `ESTABLISHED` 状态；UDP 查询所有状态。
/// 返回匹配的连接数；Netlink 通信失败时返回 IO 错误。
pub fn connections_count_with_protocol(family: u8, protocol: u8) -> io::Result<u64> {
    // 1. 构造 Netlink 消息头
    let hdr = libc::nlmsghdr {
        nlmsg_len: 0, // 先置 0，序列化时回填
        nlmsg_type: SOCK_DIAG_BY_FAMILY,
        nlmsg_flags: (libc::NLM_F_DUMP | libc::NLM_F_REQUEST) as u16,
        nlmsg_seq: 0,
        nlmsg_pid: 0,
    };

    // 2. 构造 inet_diag_req_v2 请求体
    let mut req = InetDiagReqV2 {
        family,
        protocol,
        ext: 0,
        pad: 0,
        states: ALL_TCP_STATES,
        id: InetDiagSockId {
            idiag_sport: 0,
            idiag_dport: 0,
            idiag_src: [0; 4],
            idiag_dst: [0; 4],
            idiag_if: 0,
            idiag_cookie: [0; 2],
        },
    };

    // TCP 仅查询 ESTABLISHED 状态
    if protocol == libc::IPPROTO_TCP as u8 {
        req.states = 1 << TCP_ESTABLISHED;
    }

    // 3. 序列化为 Netlink 消息（头 + 载荷）
    let msg = serialize_netlink_message(&hdr, &req)?;

    // 4. 发送请求并统计返回消息数
    netlink_inet_diag_only_count(&msg)
}

/// 发送 Netlink 请求并统计内核返回的 socket 诊断消息数。
///
/// - `request` - 已序列化的 Netlink 请求字节
///
/// 返回连接数；IO 失败时返回错误。
fn netlink_inet_diag_only_count(request: &[u8]) -> io::Result<u64> {
    let fd = unsafe { socket(libc::AF_NETLINK, libc::SOCK_RAW, libc::NETLINK_SOCK_DIAG) };
    if fd < 0 {
        let e = io::Error::last_os_error();
        error!(target: "monitoring", "Netlink 错误: 创建 socket 失败: {e}");
        return Err(e);
    }
    let _guard = FdGuard(fd);

    let mut addr: sockaddr_nl = unsafe { zeroed() };
    addr.nl_family = libc::AF_NETLINK as u16;
    addr.nl_pid = 0;
    addr.nl_groups = 0;

    // 发送请求
    let ret = unsafe {
        sendto(
            fd,
            request.as_ptr() as *const c_void,
            request.len(),
            0,
            &addr as *const sockaddr_nl as *const sockaddr,
            size_of::<sockaddr_nl>() as u32,
        )
    };
    if ret < 0 {
        let e = io::Error::last_os_error();
        error!(target: "monitoring", "Netlink 错误: sendto 失败: {e}");
        return Err(e);
    }

    // 准备读取缓冲区，按页大小分配
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page_size = if page_size > 0 {
        page_size as usize
    } else {
        4096
    };
    let mut buf: Vec<u8> = vec![0u8; page_size];

    let mut total_count: u64 = 0;

    loop {
        // 每次用整个 buf 接收，nr 为本批次有效长度
        let nr = unsafe {
            recvfrom(
                fd,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if nr < 0 {
            let e = io::Error::last_os_error();
            error!(target: "monitoring", "Netlink 错误: recvfrom 失败: {e}");
            return Err(e);
        }
        let nr = nr as usize;
        if nr < NLMSG_HDRLEN {
            // 短于头部的批次是畸形数据，跳过等待下一个正常批次或 NLMSG_DONE
            continue;
        }

        let slice = &buf[..nr];

        let (count, done) = count_netlink_messages(slice)?;
        total_count += count;
        if done {
            break;
        }
    }

    Ok(total_count)
}

/// 统计一批 Netlink 消息中的实际连接记录数。
///
/// - `b` - 从 `recvfrom` 获得的原始字节切片
///
/// 返回 `(消息数, 是否遇到 DONE/ERROR)`；遇到畸形数据时返回 IO 错误。
fn count_netlink_messages(mut b: &[u8]) -> io::Result<(u64, bool)> {
    let mut msgs: u64 = 0;
    let mut done = false;

    while b.len() >= NLMSG_HDRLEN {
        let (dlen, at_end) = netlink_message_header(b)?;
        // DONE / ERROR 是控制消息，不是实际连接记录，不应计入
        if at_end {
            done = true;
            break;
        }
        msgs += 1;
        // 防御：对齐长度为零会导致切片不前进、外层 while 死循环，直接退出
        if dlen == 0 {
            break;
        }
        b = &b[dlen..];
    }

    Ok((msgs, done))
}

/// 解析当前切片的首个 `nlmsghdr`，返回其对齐长度及是否为 DONE/ERROR。
///
/// - `b` - 原始字节切片
///
/// 使用 `ptr::read_unaligned` 读取头部，因为 `recvfrom` 返回的缓冲区无对齐保证，
/// 在严格对齐目标（如 ARMv7）上直接引用转换可能产生 SIGBUS。
fn netlink_message_header(b: &[u8]) -> io::Result<(usize, bool)> {
    if b.len() < NLMSG_HDRLEN {
        warn!(target: "monitoring", "Netlink 错误: 消息头部长度不足");
        return Err(io::Error::from_raw_os_error(libc::EINVAL));
    }

    let h: libc::nlmsghdr = unsafe { ptr::read_unaligned(b.as_ptr().cast::<libc::nlmsghdr>()) };
    let len = h.nlmsg_len as usize;
    let l = nlm_align_of(len as i32) as usize;

    if len < NLMSG_HDRLEN || l > b.len() {
        warn!(target: "monitoring", "Netlink 错误: 消息长度异常 len={len}, aligned={l}, buf_len={}", b.len());
        return Err(io::Error::from_raw_os_error(libc::EINVAL));
    }

    if h.nlmsg_type == libc::NLMSG_DONE as u16 || h.nlmsg_type == libc::NLMSG_ERROR as u16 {
        return Ok((l, true));
    }

    Ok((l, false))
}

/// 将消息长度按 4 字节对齐（Netlink 协议要求）。
#[inline]
fn nlm_align_of(msglen: i32) -> i32 {
    (msglen + libc::NLA_ALIGNTO - 1) & !(libc::NLA_ALIGNTO - 1)
}

/// 将头部和载荷序列化为完整的 Netlink 消息，回填 `nlmsg_len`。
///
/// - `hdr` - Netlink 消息头部模板
/// - `req` - `inet_diag_req_v2` 载荷
///
/// 返回序列化后的字节向量。
fn serialize_netlink_message(hdr: &libc::nlmsghdr, req: &InetDiagReqV2) -> io::Result<Vec<u8>> {
    let total = NLMSG_HDRLEN + size_of::<InetDiagReqV2>();
    let mut msg = vec![0u8; total];

    // 拷贝头部并回填 nlmsg_len
    let mut h = *hdr;
    h.nlmsg_len = total as u32;

    unsafe {
        // 头部
        ptr::copy_nonoverlapping(
            &h as *const libc::nlmsghdr as *const u8,
            msg.as_mut_ptr(),
            NLMSG_HDRLEN,
        );
        // 载荷
        ptr::copy_nonoverlapping(
            req as *const InetDiagReqV2 as *const u8,
            msg.as_mut_ptr().add(NLMSG_HDRLEN),
            size_of::<InetDiagReqV2>(),
        );
    }

    Ok(msg)
}

/// 文件描述符 RAII 守卫，drop 时自动 `close`。
struct FdGuard(RawFd);
impl Drop for FdGuard {
    fn drop(&mut self) {
        unsafe { close(self.0) };
    }
}

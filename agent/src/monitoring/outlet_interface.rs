//! 出口网卡识别模块。
//!
//! 找出流量真正进出本机的网卡（出口网卡），供流量统计和动态摘要使用。
//! 识别规则按顺序依次尝试，前一条选不出网卡时才用下一条：
//! 1. 内核判断：`/sys/devices/virtual/net` 下没有的网卡
//! 2. 容器特例：`eth*`、`venet0`
//! 3. 网卡名判断：`is_virtual_interface`

// 骨架阶段尚未接入调用方，接入后删除
#![allow(dead_code, unused_variables, clippy::needless_pass_by_ref_mut)]

use std::collections::HashMap;

/// 单块网卡的系统信息
#[derive(Debug, Clone)]
struct InterfaceFacts {
    /// 网卡名
    name: String,
    /// 网卡编号
    ifindex: Option<u32>,
    /// 是否位于 `/sys/devices/virtual/net/` 下
    is_virtual: bool,
}

/// 出口网卡识别结果缓存
#[derive(Debug, Default)]
pub struct OutletCache {
    /// (网卡名, 网卡编号) → 是否为出口网卡
    known: HashMap<(String, Option<u32>), bool>,
    /// 是否已打印过容器网络警告
    container_warned: bool,
}

impl OutletCache {
    /// 判断网卡是否为出口网卡。
    ///
    /// - `name`: 网卡名
    /// - `ifindex`: 网卡编号
    /// - 返回: 是否为出口网卡
    ///
    /// 1. 缓存中已有该 (网卡名, 网卡编号) 时直接返回
    /// 2. 否则清空缓存，读取所有网卡信息（`read_interface_facts`，非 Linux 平台或读取失败时为空）
    /// 3. 按内核判断选出口网卡（`select_by_kernel`）
    /// 4. 选不出时检查容器网络（`warn_if_container_without_host_network`），再按容器特例选（`select_by_container`）
    /// 5. 选出的网卡记为出口网卡，当前网卡不在其中则记为非出口网卡
    /// 6. 网卡信息为空时，当前网卡按网卡名判断（`is_outlet_by_name`）
    pub fn is_outlet(&mut self, name: &str, ifindex: Option<u32>) -> bool {
        todo!()
    }
}

/// 读取网卡编号。
///
/// - `name`: 网卡名
/// - 返回: Linux 读取 `/sys/class/net/<网卡>/ifindex`，其他平台或读取失败返回 `None`
pub fn read_ifindex(name: &str) -> Option<u32> {
    todo!()
}

/// 读取 `/sys/class/net` 下所有网卡的信息。
///
/// - 返回: 每块网卡的名字、编号、是否为虚拟网卡；非 Linux 平台或读取失败返回空列表
///
/// 1. 遍历 `/sys/class/net`
/// 2. 检查 `/sys/devices/virtual/net/<网卡>` 是否存在，判断是否为虚拟网卡
/// 3. 读取网卡编号
fn read_interface_facts() -> Vec<InterfaceFacts> {
    todo!()
}

/// 规则一：按内核判断，非虚拟网卡为出口网卡。
///
/// - `facts`: 所有网卡的信息
/// - 返回: 出口网卡的 (网卡名, 网卡编号)
fn select_by_kernel(facts: &[InterfaceFacts]) -> Vec<(String, Option<u32>)> {
    todo!()
}

/// 规则二：容器特例，`eth*` 和 `venet0` 为出口网卡。
///
/// - `facts`: 所有网卡的信息
/// - 返回: 出口网卡的 (网卡名, 网卡编号)
fn select_by_container(facts: &[InterfaceFacts]) -> Vec<(String, Option<u32>)> {
    todo!()
}

/// 规则三：按网卡名判断。
///
/// - `name`: 网卡名
/// - 返回: 不匹配虚拟网卡前缀（`is_virtual_interface`）时为出口网卡
fn is_outlet_by_name(name: &str) -> bool {
    todo!()
}

/// Agent 运行在容器中且未使用主机网络时，打印一次警告日志。
///
/// - `warned`: 是否已打印过，打印后置为 `true`
///
/// 1. 已打印过则直接返回
/// 2. 检查 `/.dockerenv` 或 `/run/.containerenv` 是否存在
/// 3. 存在时打印警告，提示使用 `--network host`
fn warn_if_container_without_host_network(warned: &mut bool) {
    todo!()
}

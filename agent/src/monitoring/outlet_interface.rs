//! 出口网卡识别模块。
//!
//! 找出流量真正进出本机的网卡（出口网卡），供流量统计和动态摘要使用。
//! 识别规则按顺序依次尝试，前一条选不出网卡时才用下一条：
//! 1. 内核判断：`/sys/devices/virtual/net` 下没有的网卡
//! 2. 容器特例：`eth*`、`venet0`
//! 3. 网卡名判断：`is_virtual_interface`

use log::warn;
use ng_monitoring::data_structure::is_virtual_interface;
use std::collections::HashMap;
use std::path::Path;

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
    identified_interfaces: HashMap<(String, Option<u32>), bool>,
    /// 是否已打印过容器网络警告
    container_warned: bool,
}

impl OutletCache {
    /// 识别网卡是否为出口网卡。
    ///
    /// - `name`: 网卡名
    /// - `ifindex`: 网卡编号
    /// - 返回: 是否为出口网卡
    ///
    /// 1. 缓存中已有该 (网卡名, 网卡编号) 时直接返回
    /// 2. 否则清空缓存，读取所有网卡信息（`read_interface_facts`，非 Linux 平台或读取失败时为空）
    /// 3. 网卡信息为空时，当前网卡按网卡名判断（`is_outlet_by_name`），结果写入缓存
    /// 4. 按内核判断选出口网卡（`select_by_kernel`）
    /// 5. 选不出时提示容器网络（`warn_if_container_without_physical_interface`），再按容器特例选（`select_by_container`）
    /// 6. 所有网卡的结果写入缓存：选出的记为出口网卡，其余记为非出口网卡
    /// 7. 返回当前网卡的结果；当前网卡不在网卡信息中时返回 `false`，不写入缓存
    pub fn identify_outlet(&mut self, name: &str, ifindex: Option<u32>) -> bool {
        let key = (name.to_owned(), ifindex);
        if let Some(&is_outlet) = self.identified_interfaces.get(&key) {
            return is_outlet;
        }

        self.identified_interfaces.clear();
        let facts = read_interface_facts();
        if facts.is_empty() {
            let is_outlet = is_outlet_by_name(name);
            self.identified_interfaces.insert(key, is_outlet);
            return is_outlet;
        }

        let mut selected = select_by_kernel(&facts);
        if selected.is_empty() {
            warn_if_container_without_physical_interface(&mut self.container_warned);
            selected = select_by_container(&facts);
        }

        for fact in &facts {
            self.identified_interfaces
                .insert((fact.name.clone(), fact.ifindex), false);
        }
        for outlet in selected {
            self.identified_interfaces.insert(outlet, true);
        }

        // 读取期间网卡刚好被删除或重建时可能不在其中，下次采集会重新判断
        self.identified_interfaces
            .get(&key)
            .copied()
            .unwrap_or(false)
    }
}

/// 读取网卡编号（Linux 平台）。
///
/// - `name`: 网卡名
/// - 返回: 读取 `/sys/class/net/<网卡>/ifindex`，网卡名不合法或读取失败返回 `None`
#[cfg(target_os = "linux")]
pub fn read_ifindex(name: &str) -> Option<u32> {
    // 网卡名会拼进路径，内核不允许网卡名含 `/`，这里同样拒绝，防止路径穿越
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return None;
    }
    std::fs::read_to_string(format!("/sys/class/net/{name}/ifindex"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// 读取网卡编号（Windows、macOS 等非 Linux 平台）。
///
/// 暂未实现，返回 `None`，由服务端靠"计数器变小"判断重置。
#[cfg(not(target_os = "linux"))]
pub const fn read_ifindex(_name: &str) -> Option<u32> {
    None
}

/// 读取 `/sys/class/net` 下所有网卡的信息（Linux 平台）。
///
/// - 返回: 每块网卡的名字、编号、是否为虚拟网卡；读取失败返回空列表
///
/// 1. 遍历 `/sys/class/net`，跳过不是目录的条目
/// 2. 检查 `/sys/devices/virtual/net/<网卡>` 是否存在，判断是否为虚拟网卡
/// 3. 读取网卡编号
#[cfg(target_os = "linux")]
fn read_interface_facts() -> Vec<InterfaceFacts> {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return Vec::new();
    };

    entries
        .flatten()
        // 加载 bonding 模块后会多出 `bonding_masters` 普通文件，不是网卡
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .map(|name| InterfaceFacts {
            is_virtual: Path::new("/sys/devices/virtual/net").join(&name).exists(),
            ifindex: read_ifindex(&name),
            name,
        })
        .collect()
}

/// 读取所有网卡的信息（Windows、macOS 等非 Linux 平台）。
///
/// 暂未实现，返回空列表，由调用方退回按网卡名判断。
#[cfg(not(target_os = "linux"))]
const fn read_interface_facts() -> Vec<InterfaceFacts> {
    Vec::new()
}

/// 规则一：按内核判断，非虚拟网卡为出口网卡。
///
/// - `facts`: 所有网卡的信息
/// - 返回: 出口网卡的 (网卡名, 网卡编号)
fn select_by_kernel(facts: &[InterfaceFacts]) -> Vec<(String, Option<u32>)> {
    facts
        .iter()
        .filter(|fact| !fact.is_virtual)
        .map(|fact| (fact.name.clone(), fact.ifindex))
        .collect()
}

/// 规则二：容器特例，`eth*` 和 `venet0` 为出口网卡。
///
/// - `facts`: 所有网卡的信息
/// - 返回: 出口网卡的 (网卡名, 网卡编号)
fn select_by_container(facts: &[InterfaceFacts]) -> Vec<(String, Option<u32>)> {
    facts
        .iter()
        .filter(|fact| fact.name.starts_with("eth") || fact.name == "venet0")
        .map(|fact| (fact.name.clone(), fact.ifindex))
        .collect()
}

/// 规则三：按网卡名判断。
///
/// - `name`: 网卡名
/// - 返回: 不匹配虚拟网卡前缀（`is_virtual_interface`）时为出口网卡
fn is_outlet_by_name(name: &str) -> bool {
    !is_virtual_interface(name)
}

/// Agent 运行在容器中、且未识别到物理网卡时，打印一次警告日志。
///
/// - `warned`: 是否已打印过，打印后置为 `true`
///
/// 调用方已确认未识别到物理网卡（`select_by_kernel` 选不出网卡），此处只判断是否在容器中：
/// 1. 已打印过则直接返回
/// 2. 检查 `/.dockerenv` 或 `/run/.containerenv` 是否存在
/// 3. 存在时打印警告，提示使用 `--network host`
fn warn_if_container_without_physical_interface(warned: &mut bool) {
    if *warned {
        return;
    }
    if Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists() {
        warn!(target: "monitoring", "No physical network interface detected, counting container interfaces (eth*, venet0) as outlet; if the agent runs in Docker, use --network host, otherwise only the container's own traffic is counted");
        *warned = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一块网卡的系统信息
    fn fact(name: &str, ifindex: u32, is_virtual: bool) -> InterfaceFacts {
        InterfaceFacts {
            name: name.to_owned(),
            ifindex: Some(ifindex),
            is_virtual,
        }
    }

    /// 取出选中网卡的名字
    fn names(selected: &[(String, Option<u32>)]) -> Vec<&str> {
        selected.iter().map(|(name, _)| name.as_str()).collect()
    }

    #[test]
    fn kernel_selects_physical_interface_on_kvm() {
        // KVM VPS：跑了 Docker 和 WireGuard，只有 eth0 不在 /sys/devices/virtual/net 下
        let facts = [
            fact("lo", 1, true),
            fact("eth0", 2, false),
            fact("docker0", 3, true),
            fact("veth1a2b3c", 4, true),
            fact("wg0", 5, true),
        ];
        assert_eq!(names(&select_by_kernel(&facts)), ["eth0"]);
    }

    #[test]
    fn kernel_selects_bond_members_not_bond() {
        // 独服做了网卡绑定：统计两块成员网卡，bond0 是虚拟网卡，不重复计算
        let facts = [
            fact("lo", 1, true),
            fact("eno1", 2, false),
            fact("eno2", 3, false),
            fact("bond0", 4, true),
        ];
        assert_eq!(names(&select_by_kernel(&facts)), ["eno1", "eno2"]);
    }

    #[test]
    fn kernel_selects_uplink_on_proxmox_host() {
        let facts = [
            fact("lo", 1, true),
            fact("eno1", 2, false),
            fact("vmbr0", 3, true),
            fact("tap100i0", 4, true),
            fact("fwbr100i0", 5, true),
        ];
        assert_eq!(names(&select_by_kernel(&facts)), ["eno1"]);
    }

    #[test]
    fn kernel_selects_nothing_in_container() {
        // LXC 容器里所有网卡都在 /sys/devices/virtual/net 下
        let facts = [fact("lo", 1, true), fact("eth0", 2, true)];
        assert!(select_by_kernel(&facts).is_empty());
    }

    #[test]
    fn container_selects_eth_on_lxc() {
        let facts = [fact("lo", 1, true), fact("eth0", 2, true)];
        assert_eq!(names(&select_by_container(&facts)), ["eth0"]);
    }

    #[test]
    fn container_selects_venet0_on_openvz() {
        let facts = [fact("lo", 1, true), fact("venet0", 2, true)];
        assert_eq!(names(&select_by_container(&facts)), ["venet0"]);
    }

    #[test]
    fn container_keeps_ifindex() {
        let facts = [fact("eth0", 42, true)];
        assert_eq!(select_by_container(&facts), [("eth0".to_owned(), Some(42))]);
    }

    #[test]
    fn name_rule_excludes_virtual_prefixes() {
        assert!(is_outlet_by_name("eth0"));
        assert!(is_outlet_by_name("ens3"));
        assert!(!is_outlet_by_name("lo"));
        assert!(!is_outlet_by_name("docker0"));
        assert!(!is_outlet_by_name("veth1a2b3c"));
    }
}

//! 虚拟化环境检测模块。

#[cfg(not(target_os = "windows"))]
pub async fn detect_virtualization() -> String {
    use vmaware::VM;
    let vm = VM::brand(None);
    if vm == "Unknown" {
        "Physical".to_string()
    } else {
        vm
    }
}

/// 检测虚拟化环境（Windows 平台）。
//
/// 通过 CPUID 指令检查 `HypervisorPresent` 特征位，若存在则读取
/// Hypervisor Vendor 字符串（如 "Microsoft Hv"、"VMwareVmware" 等）。
/// 非 x86/x86_64 架构无法执行 CPUID，返回 "Unknown"。
#[cfg(target_os = "windows")]
pub async fn detect_virtualization() -> String {
    {
        use raw_cpuid::CpuId;
        let hypervisor_present = {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                CpuId::new()
                    .get_feature_info()
                    .is_some_and(|f| f.has_hypervisor())
            }
            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            {
                false
            }
        };

        let hypervisor_vendor = {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                if hypervisor_present {
                    CpuId::new()
                        .get_hypervisor_info()
                        .map(|hv| format!("{:?}", hv.identify()))
                } else {
                    None
                }
            }
            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            {
                None
            }
        };

        hypervisor_vendor.unwrap_or_else(|| "Unknown".to_string())
    }
}

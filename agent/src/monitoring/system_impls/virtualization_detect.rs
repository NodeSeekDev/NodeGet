#[cfg(target_os = "linux")]
pub async fn detect_virtualization() -> String {
    heim_virt::detect()
        .await
        .unwrap_or(heim_virt::Virtualization::Unknown)
        .as_str()
        .to_string()
}

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

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub async fn detect_virtualization() -> String {
    "Unknown".to_string() // TODO: MacOS Support
}

#[cfg(target_os = "windows")]
pub fn count_processes() -> u32 {
    use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows_sys::Win32::Foundation::{FALSE, GetLastError};
    use windows_sys::Win32::System::ProcessStatus::EnumProcesses;
    // 初始缓冲区容量
    let mut mut_process_ids_capacity = 1024;

    loop {
        let mut bytes_returned: u32 = 0;
        let mut process_ids: Vec<u32> = Vec::with_capacity(mut_process_ids_capacity);

        let result = unsafe {
            EnumProcesses(
                process_ids.as_mut_ptr().cast(),
                (mut_process_ids_capacity * std::mem::size_of::<u32>()) as u32,
                &raw mut bytes_returned,
            )
        };

        if result == FALSE {
            let error_code = unsafe { GetLastError() };

            if error_code == ERROR_INSUFFICIENT_BUFFER {
                mut_process_ids_capacity += 1024; // 扩充缓冲区容量
                continue;
            }
            return 0; // 其他错误
        }

        let num_processes = bytes_returned / size_of::<u32>() as u32;
        return num_processes;
    }
}

#[cfg(target_os = "linux")]
pub fn count_processes() -> u32 {
    use std::fs;
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten() // 展开 Result<DirEntry>
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok()) // 对应 UID
                .is_some()
        })
        .count() as u32
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn count_processes() -> u32 {
    0 // TODO: MacOS Support
}

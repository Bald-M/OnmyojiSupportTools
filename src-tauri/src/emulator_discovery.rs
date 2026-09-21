use std::path::Path;

#[derive(Clone, Copy)]
#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
struct LocalTcpListener {
    address: u32,
    port: u16,
    pid: u32,
}

pub(crate) trait LocalEmulatorDiscovery: Send + Sync {
    fn discover_mumu_adb_ports(&self) -> Result<Vec<u16>, String>;
}

pub(crate) struct SystemLocalEmulatorDiscovery;

#[cfg(not(target_os = "windows"))]
impl LocalEmulatorDiscovery for SystemLocalEmulatorDiscovery {
    fn discover_mumu_adb_ports(&self) -> Result<Vec<u16>, String> {
        Ok(Vec::new())
    }
}

#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
fn is_mumu_process(path: &Path) -> bool {
    let full_path = path.to_string_lossy().to_ascii_lowercase();
    let (directory, filename) = full_path
        .rfind(['\\', '/'])
        .map(|separator| full_path.split_at(separator))
        .unwrap_or(("", full_path.as_str()));
    let filename = filename.trim_start_matches(['\\', '/']);
    (filename.contains("mumu") || filename.contains("nemu"))
        && (directory.contains("mumu") || directory.contains("netease"))
}

#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
fn filter_mumu_ports(
    listeners: &[LocalTcpListener],
    mut process_path: impl FnMut(u32) -> Option<std::path::PathBuf>,
) -> Vec<u16> {
    use std::collections::{HashMap, HashSet};

    let mut process_paths = HashMap::new();
    let mut ports = HashSet::new();
    for listener in listeners {
        if listener.address != 0 && listener.address != u32::from_ne_bytes([127, 0, 0, 1]) {
            continue;
        }
        let path = process_paths
            .entry(listener.pid)
            .or_insert_with(|| process_path(listener.pid));
        if path.as_ref().is_some_and(|path| is_mumu_process(path)) && listener.port != 0 {
            ports.insert(listener.port);
        }
    }
    let mut ports: Vec<_> = ports.into_iter().collect();
    ports.sort_unstable();
    ports
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{ffi::c_void, path::PathBuf};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, NO_ERROR},
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::AF_INET,
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
    };

    use super::{
        LocalEmulatorDiscovery, LocalTcpListener, SystemLocalEmulatorDiscovery, filter_mumu_ports,
    };

    impl LocalEmulatorDiscovery for SystemLocalEmulatorDiscovery {
        fn discover_mumu_adb_ports(&self) -> Result<Vec<u16>, String> {
            let listeners = tcp_listeners()?;
            Ok(filter_mumu_ports(&listeners, process_path))
        }
    }

    fn tcp_listeners() -> Result<Vec<LocalTcpListener>, String> {
        let mut size = 0_u32;
        // SAFETY: the first call intentionally passes a null buffer to obtain the required size.
        let first = unsafe {
            GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut size,
                0,
                AF_INET as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if first != ERROR_INSUFFICIENT_BUFFER || size < size_of::<u32>() as u32 {
            return Err(format!("读取 TCP 监听表大小失败（Windows 错误码 {first}）"));
        }

        let mut buffer = vec![0_u8; size as usize];
        // SAFETY: buffer has the exact size requested by GetExtendedTcpTable and remains alive
        // while the returned rows are copied into an owned Vec.
        let result = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut size,
                0,
                AF_INET as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if result != NO_ERROR {
            return Err(format!("读取 TCP 监听表失败（Windows 错误码 {result}）"));
        }

        let count = u32::from_ne_bytes(buffer[..4].try_into().expect("checked table header"));
        let rows_offset = align_up(size_of::<u32>(), align_of::<MIB_TCPROW_OWNER_PID>());
        let needed = rows_offset + count as usize * size_of::<MIB_TCPROW_OWNER_PID>();
        if needed > buffer.len() {
            return Err("Windows 返回的 TCP 监听表不完整".to_owned());
        }
        // SAFETY: offset is aligned for MIB_TCPROW_OWNER_PID and the bounds above cover count rows.
        let rows = unsafe {
            std::slice::from_raw_parts(
                buffer
                    .as_ptr()
                    .add(rows_offset)
                    .cast::<MIB_TCPROW_OWNER_PID>(),
                count as usize,
            )
        };
        Ok(rows
            .iter()
            .map(|row| LocalTcpListener {
                address: row.dwLocalAddr,
                port: u16::from_be(row.dwLocalPort as u16),
                pid: row.dwOwningPid,
            })
            .collect())
    }

    fn process_path(pid: u32) -> Option<PathBuf> {
        // SAFETY: OpenProcess is called with a PID supplied by the OS listener table.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut buffer = vec![0_u16; 32_768];
        let mut length = buffer.len() as u32;
        // SAFETY: buffer is writable for length UTF-16 code units and handle is closed below.
        let success =
            unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
        // SAFETY: handle was returned by OpenProcess and is closed exactly once.
        unsafe { CloseHandle(handle) };
        (success != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
    }

    const fn align_up(value: usize, alignment: usize) -> usize {
        (value + alignment - 1) & !(alignment - 1)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use std::{collections::HashMap, path::PathBuf};

    use super::{LocalTcpListener, filter_mumu_ports, is_mumu_process};

    #[test]
    fn recognizes_only_mumu_or_nemu_executables_in_vendor_paths() {
        assert!(is_mumu_process(Path::new(
            r"C:\Program Files\Netease\MuMuPlayer-12.0\shell\MuMuPlayer.exe"
        )));
        assert!(is_mumu_process(Path::new(
            r"C:\Program Files\MuMuVMMVbox\Hypervisor\MuMuVMMHeadless.exe"
        )));
        assert!(!is_mumu_process(Path::new(r"C:\Temp\mumu-helper.exe")));
        assert!(!is_mumu_process(Path::new(
            r"C:\Program Files\Netease\other.exe"
        )));
    }

    #[test]
    fn filters_local_listeners_by_owning_mumu_process_and_deduplicates_ports() {
        let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
        let listeners = [
            LocalTcpListener {
                address: loopback,
                port: 16384,
                pid: 10,
            },
            LocalTcpListener {
                address: 0,
                port: 16384,
                pid: 10,
            },
            LocalTcpListener {
                address: loopback,
                port: 16416,
                pid: 11,
            },
            LocalTcpListener {
                address: loopback,
                port: 5555,
                pid: 12,
            },
            LocalTcpListener {
                address: u32::from_ne_bytes([192, 168, 1, 2]),
                port: 16448,
                pid: 10,
            },
        ];
        let paths = HashMap::from([
            (
                10,
                PathBuf::from(r"C:\Program Files\Netease\MuMuPlayer-12.0\MuMuPlayer.exe"),
            ),
            (
                11,
                PathBuf::from(r"C:\Program Files\MuMuVMMVbox\MuMuVMMHeadless.exe"),
            ),
            (12, PathBuf::from(r"C:\Windows\System32\other.exe")),
        ]);

        let ports = filter_mumu_ports(&listeners, |pid| paths.get(&pid).cloned());

        assert_eq!(ports, vec![16384, 16416]);
    }
}

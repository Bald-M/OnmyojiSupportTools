use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredMuMuInstance {
    pub port: u16,
    pub display_name: Option<String>,
}

impl DiscoveredMuMuInstance {
    pub(crate) fn new(port: u16, display_name: Option<String>) -> Self {
        Self { port, display_name }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MuMuDiscovery {
    pub instances: Vec<DiscoveredMuMuInstance>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy)]
#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
struct LocalTcpListener {
    address: u32,
    port: u16,
    pid: u32,
}

#[async_trait]
#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
trait MuMuManagerRunner: Send + Sync {
    async fn query(&self, path: &Path, args: &[&str]) -> Result<Vec<u8>, String>;
}

#[async_trait]
pub(crate) trait LocalEmulatorDiscovery: Send + Sync {
    async fn discover_mumu_instances(&self) -> Result<MuMuDiscovery, String>;
}

pub(crate) struct SystemLocalEmulatorDiscovery;

#[cfg(not(target_os = "windows"))]
#[async_trait]
impl LocalEmulatorDiscovery for SystemLocalEmulatorDiscovery {
    async fn discover_mumu_instances(&self) -> Result<MuMuDiscovery, String> {
        Ok(MuMuDiscovery::default())
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
fn mumu_listeners(
    listeners: &[LocalTcpListener],
    mut process_path: impl FnMut(u32) -> Option<std::path::PathBuf>,
) -> Vec<(LocalTcpListener, PathBuf)> {
    let mut process_paths = HashMap::new();
    let mut discovered = Vec::new();
    for listener in listeners {
        if listener.address != 0 && listener.address != u32::from_ne_bytes([127, 0, 0, 1]) {
            continue;
        }
        let path = process_paths
            .entry(listener.pid)
            .or_insert_with(|| process_path(listener.pid));
        if let Some(path) = path.as_ref().filter(|path| is_mumu_process(path)) {
            discovered.push((*listener, path.clone()));
        }
    }
    discovered
}

#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
fn discover_mumu_listener_fallback(
    listeners: &[(LocalTcpListener, PathBuf)],
) -> Vec<DiscoveredMuMuInstance> {
    let ports: HashSet<_> = listeners
        .iter()
        .filter_map(|(listener, _)| (listener.port != 0).then_some(listener.port))
        .collect();
    let mut instances: Vec<_> = ports
        .into_iter()
        .map(|port| DiscoveredMuMuInstance::new(port, None))
        .collect();
    instances.sort_unstable_by_key(|instance| instance.port);
    instances
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn mumu_manager_paths(
    listeners: &[(LocalTcpListener, PathBuf)],
    mut candidate_exists: impl FnMut(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut seen_directories = HashSet::new();
    let mut paths = HashSet::new();
    for (_, process) in listeners {
        let Some(directory) = process.parent() else {
            continue;
        };
        if !seen_directories.insert(directory) {
            continue;
        }
        for candidate in [
            directory.join("MuMuManager.exe"),
            directory.join("..").join("shell").join("MuMuManager.exe"),
            directory.join("..").join("nx_main").join("MuMuManager.exe"),
        ] {
            if candidate_exists(&candidate) {
                paths.insert(candidate);
            }
        }
    }
    let mut paths: Vec<_> = paths.into_iter().collect();
    paths.sort_unstable();
    paths
}

#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
async fn discover_from_mumu_managers(
    managers: &[PathBuf],
    fallback: Vec<DiscoveredMuMuInstance>,
    runner: &dyn MuMuManagerRunner,
) -> MuMuDiscovery {
    let mut warnings = Vec::new();
    for manager in managers {
        match runner.query(manager, &["info", "-v", "all"]).await {
            Ok(output) => match parse_mumu_manager_info(&output) {
                Ok(instances) if !instances.is_empty() || fallback.is_empty() => {
                    return MuMuDiscovery {
                        instances,
                        warnings,
                    };
                }
                Ok(_) => warnings.push(format!(
                    "{} 未返回运行中的 MuMu 实例，已回退到端口发现",
                    manager.display()
                )),
                Err(error) => warnings.push(format!("{}：{error}", manager.display())),
            },
            Err(error) => warnings.push(error),
        }
    }
    MuMuDiscovery {
        instances: fallback,
        warnings,
    }
}

#[derive(Deserialize)]
#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
struct MuMuManagerInstance {
    name: String,
    adb_port: Option<u16>,
    #[serde(default)]
    is_process_started: bool,
    #[serde(default)]
    is_android_started: bool,
}

#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
fn parse_mumu_manager_info(bytes: &[u8]) -> Result<Vec<DiscoveredMuMuInstance>, String> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let instances: HashMap<String, MuMuManagerInstance> = serde_json::from_slice(bytes)
        .map_err(|error| format!("MuMuManager 返回无效 JSON：{error}"))?;
    let mut discovered: Vec<_> = instances
        .into_values()
        .filter(|instance| instance.is_process_started && instance.is_android_started)
        .filter_map(|instance| {
            instance
                .adb_port
                .map(|port| DiscoveredMuMuInstance::new(port, Some(instance.name)))
        })
        .collect();
    discovered.sort_unstable_by_key(|instance| instance.port);
    discovered.dedup_by_key(|instance| instance.port);
    Ok(discovered)
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{
        ffi::c_void,
        path::{Path, PathBuf},
        process::Stdio,
    };

    use async_trait::async_trait;
    use tokio::time::timeout;

    use crate::process::background_command;

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
        LocalEmulatorDiscovery, LocalTcpListener, MuMuDiscovery, MuMuManagerRunner,
        SystemLocalEmulatorDiscovery, discover_from_mumu_managers, discover_mumu_listener_fallback,
        mumu_listeners, mumu_manager_paths,
    };

    struct SystemMuMuManagerRunner;

    #[async_trait]
    impl MuMuManagerRunner for SystemMuMuManagerRunner {
        async fn query(&self, path: &std::path::Path, args: &[&str]) -> Result<Vec<u8>, String> {
            let mut command = background_command(path);
            command
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            let output = timeout(std::time::Duration::from_secs(8), command.output())
                .await
                .map_err(|_| format!("{} 查询超时", path.display()))?
                .map_err(|error| format!("无法启动 {}：{error}", path.display()))?;
            if !output.status.success() {
                return Err(format!("{} 查询失败", path.display()));
            }
            Ok(output.stdout)
        }
    }

    #[async_trait]
    impl LocalEmulatorDiscovery for SystemLocalEmulatorDiscovery {
        async fn discover_mumu_instances(&self) -> Result<MuMuDiscovery, String> {
            let listeners = tcp_listeners()?;
            let mumu_listeners = mumu_listeners(&listeners, process_path);
            let fallback = discover_mumu_listener_fallback(&mumu_listeners);
            let managers = mumu_manager_paths(&mumu_listeners, Path::is_file);
            Ok(discover_from_mumu_managers(&managers, fallback, &SystemMuMuManagerRunner).await)
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
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use async_trait::async_trait;

    use super::{
        DiscoveredMuMuInstance, LocalTcpListener, MuMuManagerRunner, discover_from_mumu_managers,
        discover_mumu_listener_fallback, is_mumu_process, mumu_listeners, mumu_manager_paths,
        parse_mumu_manager_info,
    };

    struct FakeManagerRunner {
        results: Mutex<Vec<Result<Vec<u8>, String>>>,
        calls: Mutex<Vec<(PathBuf, Vec<String>)>>,
    }

    #[async_trait]
    impl MuMuManagerRunner for FakeManagerRunner {
        async fn query(&self, path: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
            self.calls.lock().unwrap().push((
                path.to_owned(),
                args.iter().map(|arg| (*arg).to_owned()).collect(),
            ));
            self.results.lock().unwrap().remove(0)
        }
    }

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

        let listeners = mumu_listeners(&listeners, |pid| paths.get(&pid).cloned());
        let instances = discover_mumu_listener_fallback(&listeners);

        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].port, 16384);
        assert_eq!(instances[0].display_name, None);
        assert_eq!(instances[1].port, 16416);
        assert_eq!(instances[1].display_name, None);
    }

    #[test]
    fn parses_only_running_android_instances_from_mumu_manager_info() {
        let output = r#"{
            "0": {"name":"MuMu安卓设备","adb_port":16384,"is_process_started":true,"is_android_started":true},
            "1": {"name":"未开机","is_process_started":false,"is_android_started":false},
            "3": {"name":"猫猫火鸡面01","adb_port":16480,"is_process_started":true,"is_android_started":true},
            "4": {"name":"启动中","adb_port":16512,"is_process_started":true,"is_android_started":false}
        }"#;

        let instances = parse_mumu_manager_info(output.as_bytes()).unwrap();

        assert_eq!(
            instances,
            vec![
                super::DiscoveredMuMuInstance::new(16384, Some("MuMu安卓设备".to_owned())),
                super::DiscoveredMuMuInstance::new(16480, Some("猫猫火鸡面01".to_owned())),
            ]
        );
    }

    #[test]
    fn selects_existing_manager_candidates_once_in_stable_order() {
        let listeners = vec![
            (
                LocalTcpListener {
                    address: 0,
                    port: 1,
                    pid: 10,
                },
                PathBuf::from(r"C:\MuMu\shell\player.exe"),
            ),
            (
                LocalTcpListener {
                    address: 0,
                    port: 2,
                    pid: 10,
                },
                PathBuf::from(r"C:\MuMu\shell\player.exe"),
            ),
        ];

        let paths = mumu_manager_paths(&listeners, |path| {
            path.ends_with("MuMuManager.exe") && !path.to_string_lossy().contains("nx_main")
        });

        assert_eq!(paths.len(), 2);
        assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[tokio::test]
    async fn manager_query_uses_documented_args_and_returns_named_instances() {
        let runner = FakeManagerRunner {
            results: Mutex::new(vec![Ok(r#"{"0":{"name":"猫猫火鸡面01","adb_port":16384,"is_process_started":true,"is_android_started":true}}"#.as_bytes().to_vec())]),
            calls: Mutex::new(Vec::new()),
        };
        let manager = PathBuf::from(r"C:\MuMu\MuMuManager.exe");

        let discovery =
            discover_from_mumu_managers(std::slice::from_ref(&manager), Vec::new(), &runner).await;

        assert_eq!(
            discovery.instances,
            vec![DiscoveredMuMuInstance::new(
                16384,
                Some("猫猫火鸡面01".to_owned())
            )]
        );
        assert!(discovery.warnings.is_empty());
        assert_eq!(
            runner.calls.into_inner().unwrap(),
            vec![(
                manager,
                vec!["info".to_owned(), "-v".to_owned(), "all".to_owned()]
            )]
        );
    }

    #[tokio::test]
    async fn manager_failures_and_invalid_output_warn_then_fall_back() {
        let runner = FakeManagerRunner {
            results: Mutex::new(vec![Err("查询超时".to_owned()), Ok(b"not json".to_vec())]),
            calls: Mutex::new(Vec::new()),
        };
        let fallback = vec![DiscoveredMuMuInstance::new(16384, None)];

        let discovery = discover_from_mumu_managers(
            &[PathBuf::from("first.exe"), PathBuf::from("second.exe")],
            fallback.clone(),
            &runner,
        )
        .await;

        assert_eq!(discovery.instances, fallback);
        assert_eq!(discovery.warnings.len(), 2);
        assert!(discovery.warnings[0].contains("查询超时"));
        assert!(discovery.warnings[1].contains("无效 JSON"));
        assert_eq!(runner.calls.into_inner().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn empty_manager_result_warns_and_uses_listener_fallback() {
        let runner = FakeManagerRunner {
            results: Mutex::new(vec![Ok(b"{}".to_vec())]),
            calls: Mutex::new(Vec::new()),
        };
        let fallback = vec![DiscoveredMuMuInstance::new(16416, None)];

        let discovery =
            discover_from_mumu_managers(&[PathBuf::from("manager.exe")], fallback.clone(), &runner)
                .await;

        assert_eq!(discovery.instances, fallback);
        assert_eq!(discovery.warnings.len(), 1);
        assert!(discovery.warnings[0].contains("未返回运行中的 MuMu 实例"));
    }
}

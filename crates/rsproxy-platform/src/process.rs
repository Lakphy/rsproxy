use crate::{PlatformError, PlatformResult};
use std::process::Command;

/// Parses a pidfile value and rejects non-numeric, zero, and init-process identifiers.
pub fn parse_pid(input: &str) -> PlatformResult<u32> {
    let pid = input.trim().parse::<u32>().map_err(|_| {
        PlatformError::InvalidState("pidfile does not contain a numeric process id".to_string())
    })?;
    if pid <= 1 {
        return Err(PlatformError::InvalidState(
            "pidfile process id must be greater than one".to_string(),
        ));
    }
    Ok(pid)
}

#[cfg(unix)]
/// Configures a child command to start in a new session after fork and before exec.
///
/// The change takes effect when the command is spawned; failure of `setsid` fails that spawn.
pub fn detach_daemon(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // SAFETY: the callback performs only the async-signal-safe `setsid` syscall
    // between fork and exec and does not access shared process state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
/// Configures a child command as a detached process in a new process group.
pub fn detach_daemon(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};

    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}

#[cfg(all(not(unix), not(windows)))]
/// Leaves child process settings unchanged on platforms without a supported detach mechanism.
pub fn detach_daemon(_command: &mut Command) {}

#[cfg(unix)]
/// Tests whether `pid` exists or is inaccessible to the caller without sending a signal.
pub fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only queries the kernel for this validated numeric pid;
    // no pointers or Rust-managed memory cross the FFI boundary.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(unix)]
/// Sends `SIGTERM` to `pid`, treating an already absent process as successfully terminated.
///
/// Callers must ensure the identifier still belongs to the intended process.
pub fn terminate_process(pid: u32) -> PlatformResult<()> {
    let pid = i32::try_from(pid)
        .map_err(|_| PlatformError::InvalidState(format!("invalid process id {pid}")))?;
    // SAFETY: SIGTERM is sent to a validated positive numeric pid and the call
    // neither dereferences pointers nor aliases Rust-managed memory.
    let result = unsafe { libc::kill(pid, libc::SIGTERM) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(PlatformError::Io {
            context: format!("terminate process {pid}"),
            source: std::io::Error::last_os_error(),
        })
    }
}

#[cfg(windows)]
/// Tests whether `pid` names a running process using a query-only process handle.
pub fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, GetLastError, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    // SAFETY: OpenProcess receives scalar access flags and a numeric pid; the
    // returned handle is checked for null and closed on the success path.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        // SAFETY: GetLastError has no preconditions and is called immediately
        // after the failed Win32 API call on this thread.
        return unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
    }
    // SAFETY: `handle` is non-null and owned by this function until CloseHandle.
    let alive = unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT;
    // SAFETY: `handle` is a valid owned process handle and is closed exactly once.
    unsafe { CloseHandle(handle) };
    alive
}

#[cfg(windows)]
/// Terminates `pid` immediately through the Windows process API.
///
/// Callers must ensure the identifier still belongs to the intended process.
pub fn terminate_process(pid: u32) -> PlatformResult<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

    // SAFETY: OpenProcess receives scalar access flags and a numeric pid; the
    // returned handle is checked for null and closed on the success path.
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle.is_null() {
        return Err(PlatformError::Io {
            context: format!("open process {pid} for termination"),
            source: std::io::Error::last_os_error(),
        });
    }
    // SAFETY: `handle` is a non-null process handle opened with PROCESS_TERMINATE.
    let result = unsafe { TerminateProcess(handle, 0) };
    let error = (result == 0).then(std::io::Error::last_os_error);
    // SAFETY: `handle` is a valid owned process handle and is closed exactly once.
    unsafe { CloseHandle(handle) };
    if let Some(error) = error {
        Err(PlatformError::Io {
            context: format!("terminate process {pid}"),
            source: error,
        })
    } else {
        Ok(())
    }
}

#[cfg(all(not(unix), not(windows)))]
/// Returns `false` where process liveness inspection is unsupported.
pub fn process_alive(_pid: u32) -> bool {
    false
}

#[cfg(all(not(unix), not(windows)))]
/// Reports that graceful process termination is unsupported on this target.
pub fn terminate_process(pid: u32) -> PlatformResult<()> {
    Err(PlatformError::Unsupported(format!(
        "process termination is unsupported for pid {pid}"
    )))
}

#[cfg(unix)]
/// Sends `SIGKILL` to `pid`, treating an already absent process as successfully terminated.
///
/// Callers must ensure the identifier still belongs to the intended process.
pub fn force_terminate_process(pid: u32) -> PlatformResult<()> {
    let pid = i32::try_from(pid)
        .map_err(|_| PlatformError::InvalidState(format!("invalid process id {pid}")))?;
    // SAFETY: SIGKILL is sent to a validated positive numeric pid and the call
    // neither dereferences pointers nor aliases Rust-managed memory.
    let result = unsafe { libc::kill(pid, libc::SIGKILL) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(PlatformError::Io {
            context: format!("force terminate process {pid}"),
            source: std::io::Error::last_os_error(),
        })
    }
}

#[cfg(windows)]
/// Immediately terminates `pid`; Windows uses the same primitive as [`terminate_process`].
pub fn force_terminate_process(pid: u32) -> PlatformResult<()> {
    terminate_process(pid)
}

#[cfg(all(not(unix), not(windows)))]
/// Reports that forced process termination is unsupported on this target.
pub fn force_terminate_process(pid: u32) -> PlatformResult<()> {
    Err(PlatformError::Unsupported(format!(
        "forced process termination is unsupported for pid {pid}"
    )))
}

#[cfg(target_os = "macos")]
/// Returns the current resident set size for `pid` in KiB, or `None` when unavailable.
pub fn resident_kib(pid: u32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = size_of::<libc::proc_taskinfo>();
    // SAFETY: proc_pidinfo receives a correctly sized writable proc_taskinfo;
    // the return-size check proves initialization before assume_init.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if written as usize != size {
        return None;
    }
    // SAFETY: the exact-size return check above proves that proc_pidinfo fully
    // initialized the proc_taskinfo value.
    Some(unsafe { info.assume_init() }.pti_resident_size / 1024)
}

#[cfg(target_os = "linux")]
/// Reads `/proc/<pid>/status` and returns the current resident set size in KiB.
///
/// Returns `None` if procfs is unavailable, the process has exited, or the value cannot be parsed.
pub fn resident_kib(pid: u32) -> Option<u64> {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(windows)]
/// Returns the current process working set for `pid` in KiB, or `None` when unavailable.
pub fn resident_kib(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    // SAFETY: OpenProcess receives scalar access flags and a numeric pid; the
    // returned handle is checked for null and closed before returning.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        return None;
    }

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..PROCESS_MEMORY_COUNTERS::default()
    };
    // SAFETY: `handle` is valid and `counters` is a writable, correctly sized
    // PROCESS_MEMORY_COUNTERS whose lifetime spans the call.
    let result = unsafe {
        GetProcessMemoryInfo(
            handle,
            &raw mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    // SAFETY: `handle` is a valid owned process handle and is closed exactly once.
    unsafe { CloseHandle(handle) };
    (result != 0).then_some((counters.WorkingSetSize / 1024) as u64)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
/// Queries `ps` for the current resident set size of `pid` in KiB.
///
/// Returns `None` if the command fails or its output cannot be parsed.
pub fn resident_kib(pid: u32) -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

#[cfg(target_os = "macos")]
/// Returns the pid of the process holding a TCP listener on `host:port`, if any.
///
/// Uses `lsof`, which ships with macOS; returns `None` when nothing is listening or `lsof`
/// is unavailable. This is a cold recovery path (orphan cleanup), not a hot code path.
pub fn pid_listening_on(host: &str, port: u16) -> Option<u32> {
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP@{host}:{port}"), "-sTCP:LISTEN", "-t"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().parse::<u32>().ok())
}

#[cfg(target_os = "linux")]
/// Returns the pid of the process holding a TCP listener on `port`, if any.
///
/// Reads `/proc/net/tcp{,6}` for a listening socket on `port`, then scans `/proc/<pid>/fd`
/// for the owning inode. `host` is accepted for signature parity but not matched, since only
/// one process can hold a listener on a given port.
pub fn pid_listening_on(_host: &str, port: u16) -> Option<u32> {
    use std::collections::HashSet;

    let inodes = listening_socket_inodes(port);
    if inodes.is_empty() {
        return None;
    }
    let targets: HashSet<String> = inodes
        .iter()
        .map(|inode| format!("socket:[{inode}]"))
        .collect();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(descriptors) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for descriptor in descriptors.flatten() {
            if let Ok(link) = std::fs::read_link(descriptor.path())
                && link.to_str().is_some_and(|value| targets.contains(value))
            {
                return Some(pid);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
/// Collects the socket inodes of TCP listeners bound to `port` across IPv4 and IPv6.
fn listening_socket_inodes(port: u16) -> Vec<u64> {
    const LISTEN_STATE: &str = "0A";

    let mut inodes = Vec::new();
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in contents.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() <= 9 || fields[3] != LISTEN_STATE {
                continue;
            }
            let Some((_, hex_port)) = fields[1].rsplit_once(':') else {
                continue;
            };
            if u16::from_str_radix(hex_port, 16).ok() != Some(port) {
                continue;
            }
            if let Ok(inode) = fields[9].parse::<u64>() {
                inodes.push(inode);
            }
        }
    }
    inodes
}

#[cfg(windows)]
/// Returns the pid of the process holding a TCP listener on `port`, if any.
///
/// Queries the IPv4 owner-pid listener table; `host` is accepted for signature parity but not
/// matched, since only one process can hold a listener on a given port.
pub fn pid_listening_on(_host: &str, port: u16) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };

    // AF_INET; used as a literal to avoid pulling in the WinSock feature.
    const AF_INET: u32 = 2;

    let mut size: u32 = 0;
    // SAFETY: a null buffer with zero size only writes the required byte count to `size`.
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    // SAFETY: `buffer` is `size` bytes and writable for the whole call.
    let result = unsafe {
        GetExtendedTcpTable(
            buffer.as_mut_ptr().cast(),
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if result != 0 {
        return None;
    }
    // SAFETY: on success the buffer starts with a MIB_TCPTABLE_OWNER_PID header whose
    // `dwNumEntries` bounds the trailing row array.
    let table = unsafe { &*(buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };
    rows.iter()
        .find(|row| u16::from_be((row.dwLocalPort & 0xFFFF) as u16) == port)
        .map(|row| row.dwOwningPid)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
/// Reports that port-owner inspection is unsupported on this target.
pub fn pid_listening_on(_host: &str, _port: u16) -> Option<u32> {
    None
}

#[cfg(target_os = "macos")]
/// Returns the executable path of `pid`, or `None` when it cannot be read.
pub fn process_executable_path(pid: u32) -> Option<std::path::PathBuf> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: proc_pidpath writes at most `buffer.len()` bytes into the owned buffer and
    // returns the number written; no Rust-managed memory is aliased across the call.
    let written =
        unsafe { libc::proc_pidpath(pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if written <= 0 {
        return None;
    }
    buffer.truncate(written as usize);
    Some(std::path::PathBuf::from(
        String::from_utf8_lossy(&buffer).into_owned(),
    ))
}

#[cfg(target_os = "linux")]
/// Returns the executable path of `pid`, or `None` when it cannot be read.
pub fn process_executable_path(pid: u32) -> Option<std::path::PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(windows)]
/// Returns the executable path of `pid`, or `None` when it cannot be read.
pub fn process_executable_path(pid: u32) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };

    // SAFETY: OpenProcess receives scalar access flags and a numeric pid; the returned
    // handle is checked for null and closed on every path below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    // SAFETY: `handle` is valid; `buffer`/`length` describe a writable region for the call.
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    // SAFETY: `handle` is a valid owned process handle closed exactly once.
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return None;
    }
    buffer.truncate(length as usize);
    Some(std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &buffer,
    )))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
/// Reports that executable-path inspection is unsupported on this target.
pub fn process_executable_path(_pid: u32) -> Option<std::path::PathBuf> {
    None
}

#[cfg(unix)]
/// Selects a Unix-domain control socket path for the given storage root.
///
/// Paths up to 96 display bytes remain under `storage/run`; longer paths use a deterministic per-user `/tmp` name.
pub fn unix_control_socket_path(storage: &std::path::Path) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};

    let local = storage.join("run/ctl.sock");
    if local.to_string_lossy().len() <= 96 {
        return local;
    }
    let digest = Sha256::digest(storage.to_string_lossy().as_bytes());
    let suffix = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    // SAFETY: geteuid has no preconditions and returns a scalar user id.
    std::path::PathBuf::from("/tmp").join(format!("rsproxy-{}-{suffix}.sock", unsafe {
        libc::geteuid()
    }))
}

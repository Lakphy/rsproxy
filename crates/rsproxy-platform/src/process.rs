use crate::{PlatformError, PlatformResult};
use std::process::Command;

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
pub fn detach_daemon(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};

    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}

#[cfg(all(not(unix), not(windows)))]
pub fn detach_daemon(_command: &mut Command) {}

#[cfg(unix)]
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
pub fn process_alive(_pid: u32) -> bool {
    false
}

#[cfg(all(not(unix), not(windows)))]
pub fn terminate_process(pid: u32) -> PlatformResult<()> {
    Err(PlatformError::Unsupported(format!(
        "process termination is unsupported for pid {pid}"
    )))
}

#[cfg(unix)]
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
pub fn force_terminate_process(pid: u32) -> PlatformResult<()> {
    terminate_process(pid)
}

#[cfg(all(not(unix), not(windows)))]
pub fn force_terminate_process(pid: u32) -> PlatformResult<()> {
    Err(PlatformError::Unsupported(format!(
        "forced process termination is unsupported for pid {pid}"
    )))
}

#[cfg(target_os = "macos")]
pub fn resident_kib(pid: u32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_taskinfo>();
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
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..PROCESS_MEMORY_COUNTERS::default()
    };
    // SAFETY: `handle` is valid and `counters` is a writable, correctly sized
    // PROCESS_MEMORY_COUNTERS whose lifetime spans the call.
    let result = unsafe {
        GetProcessMemoryInfo(
            handle,
            &raw mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    // SAFETY: `handle` is a valid owned process handle and is closed exactly once.
    unsafe { CloseHandle(handle) };
    (result != 0).then_some((counters.WorkingSetSize / 1024) as u64)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn resident_kib(pid: u32) -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

#[cfg(unix)]
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

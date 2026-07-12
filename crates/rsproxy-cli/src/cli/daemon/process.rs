use std::process::Command;

pub(super) fn parse_pid(input: &str) -> Result<u32, String> {
    let pid = input
        .trim()
        .parse::<u32>()
        .map_err(|_| "pidfile does not contain a numeric process id".to_string())?;
    if pid <= 1 {
        return Err("pidfile process id must be greater than one".to_string());
    }
    Ok(pid)
}

#[cfg(unix)]
pub(super) fn detach_daemon(command: &mut Command) {
    use std::os::unix::process::CommandExt;

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
pub(super) fn detach_daemon(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};

    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}

#[cfg(all(not(unix), not(windows)))]
pub(super) fn detach_daemon(_command: &mut Command) {}

#[cfg(unix)]
pub(super) fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(unix)]
pub(super) fn terminate_process(pid: u32) -> Result<(), String> {
    let pid = i32::try_from(pid).map_err(|_| format!("invalid process id {pid}"))?;
    let result = unsafe { libc::kill(pid, libc::SIGTERM) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!(
            "terminate process {pid}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(windows)]
pub(super) fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, GetLastError, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
    }
    let alive = unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT;
    unsafe { CloseHandle(handle) };
    alive
}

#[cfg(windows)]
pub(super) fn terminate_process(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle.is_null() {
        return Err(format!(
            "open process {pid} for termination: {}",
            std::io::Error::last_os_error()
        ));
    }
    let result = unsafe { TerminateProcess(handle, 0) };
    unsafe { CloseHandle(handle) };
    if result == 0 {
        Err(format!(
            "terminate process {pid}: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(all(not(unix), not(windows)))]
pub(super) fn process_alive(_pid: u32) -> bool {
    false
}

#[cfg(all(not(unix), not(windows)))]
pub(super) fn terminate_process(pid: u32) -> Result<(), String> {
    Err(format!("process termination is unsupported for pid {pid}"))
}

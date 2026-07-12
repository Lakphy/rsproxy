use std::io::{self, Read, Write};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ,
    GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile,
    WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    WaitNamedPipeW,
};

const PIPE_BUFFER_BYTES: u32 = 64 * 1024;
const PIPE_CONNECT_TIMEOUT_MS: u32 = 5_000;

pub(crate) struct NamedPipeListener {
    path: String,
    next: Option<NamedPipeStream>,
}

impl NamedPipeListener {
    pub(crate) fn bind(path: &str) -> io::Result<Self> {
        let path = canonical_path(path);
        let next = Some(NamedPipeStream::server(&path, true)?);
        Ok(Self { path, next })
    }

    pub(crate) fn endpoint(&self) -> String {
        format!("pipe:{}", self.path)
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn accept(&mut self) -> io::Result<NamedPipeStream> {
        let stream = self
            .next
            .take()
            .ok_or_else(|| io::Error::other("named pipe listener has no pending instance"))?;
        let connected = unsafe { ConnectNamedPipe(stream.handle, std::ptr::null_mut()) };
        if connected == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_PIPE_CONNECTED {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
        self.next = Some(NamedPipeStream::server(&self.path, false)?);
        Ok(stream)
    }
}

pub(crate) struct NamedPipeStream {
    handle: HANDLE,
    server: bool,
}

unsafe impl Send for NamedPipeStream {}

impl NamedPipeStream {
    fn server(path: &str, first: bool) -> io::Result<Self> {
        let wide = wide(path);
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX
                    | if first {
                        FILE_FLAG_FIRST_PIPE_INSTANCE
                    } else {
                        0
                    },
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                PIPE_BUFFER_BYTES,
                PIPE_BUFFER_BYTES,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle,
            server: true,
        })
    }

    pub(crate) fn connect(path: &str) -> io::Result<Self> {
        let path = canonical_path(path);
        let wide = wide(&path);
        loop {
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(Self {
                    handle,
                    server: false,
                });
            }
            let error = unsafe { GetLastError() };
            if error != ERROR_PIPE_BUSY
                || unsafe { WaitNamedPipeW(wide.as_ptr(), PIPE_CONNECT_TIMEOUT_MS) } == 0
            {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
    }
}

impl Read for NamedPipeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut read = 0u32;
        let length = buffer.len().min(u32::MAX as usize) as u32;
        let result = unsafe {
            ReadFile(
                self.handle,
                buffer.as_mut_ptr(),
                length,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if result != 0 {
            return Ok(read as usize);
        }
        let error = unsafe { GetLastError() };
        if error == ERROR_BROKEN_PIPE {
            Ok(0)
        } else {
            Err(io::Error::from_raw_os_error(error as i32))
        }
    }
}

impl Write for NamedPipeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut written = 0u32;
        let length = buffer.len().min(u32::MAX as usize) as u32;
        let result = unsafe {
            WriteFile(
                self.handle,
                buffer.as_ptr(),
                length,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(written as usize)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for NamedPipeStream {
    fn drop(&mut self) {
        if self.server {
            unsafe { DisconnectNamedPipe(self.handle) };
        }
        unsafe { CloseHandle(self.handle) };
    }
}

fn canonical_path(path: &str) -> String {
    if path.starts_with(r"\\.\pipe\") {
        path.to_string()
    } else {
        format!(r"\\.\pipe\{path}")
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

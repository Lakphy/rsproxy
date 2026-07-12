use std::fs::{self, File};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) fn rsproxy_binary() -> PathBuf {
    std::env::var_os("RSPROXY_ACCEPTANCE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_rsproxy")))
}

pub(super) struct TempStorage {
    path: PathBuf,
}

impl TempStorage {
    pub(super) fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rsproxy-large-stream-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempStorage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) struct Daemon {
    child: Child,
    log_path: PathBuf,
}

impl Daemon {
    pub(super) fn spawn(storage: &Path, proxy: SocketAddr, api: SocketAddr) -> Self {
        let log_path = storage.join("acceptance.log");
        let log = File::create(&log_path).unwrap();
        let log_err = log.try_clone().unwrap();
        let child = Command::new(rsproxy_binary())
            .args([
                "run",
                "--host",
                "127.0.0.1",
                "--port",
                &proxy.port().to_string(),
                "--api",
                &api.to_string(),
                "--trace-body-limit",
                "4096",
                "--trace-mem-budget",
                "64mb",
                "--trace-disk-budget",
                "0",
                "--no-mitm",
                "--storage",
            ])
            .arg(storage)
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .unwrap();
        Self { child, log_path }
    }

    pub(super) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(super) fn wait_until_ready(&mut self, address: SocketAddr) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_ok() {
                return;
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                panic!(
                    "rsproxy exited with {status}: {}",
                    fs::read_to_string(&self.log_path).unwrap_or_default()
                );
            }
            assert!(
                Instant::now() < deadline,
                "rsproxy was not ready: {}",
                fs::read_to_string(&self.log_path).unwrap_or_default()
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) struct RssMonitor {
    stop: Arc<AtomicBool>,
    peak_kib: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
}

impl RssMonitor {
    pub(super) fn start(pid: u32, baseline_kib: u64) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak_kib = Arc::new(AtomicU64::new(baseline_kib));
        let worker_stop = Arc::clone(&stop);
        let worker_peak = Arc::clone(&peak_kib);
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                if let Some(rss) = resident_kib(pid) {
                    worker_peak.fetch_max(rss, Ordering::Relaxed);
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        Self {
            stop,
            peak_kib,
            worker: Some(worker),
        }
    }

    pub(super) fn stop(mut self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
        self.peak_kib.load(Ordering::Relaxed)
    }
}

#[cfg(target_os = "macos")]
pub(super) fn resident_kib(pid: u32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_taskinfo>();
    // proc_pidinfo writes exactly one proc_taskinfo for a live process.
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
    // The size check above proves the structure was initialized by the kernel.
    Some(unsafe { info.assume_init() }.pti_resident_size / 1024)
}

#[cfg(target_os = "linux")]
pub(super) fn resident_kib(pid: u32) -> Option<u64> {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(super) fn resident_kib(pid: u32) -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

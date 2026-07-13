use serde_json::Value;
use std::fs;
use std::net::TcpListener;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) struct DaemonHarness {
    pub(super) storage: PathBuf,
    proxy_port: u16,
    api_port: u16,
}

impl DaemonHarness {
    pub(super) fn new() -> Self {
        let proxy_port = unused_port();
        let mut api_port = unused_port();
        while api_port == proxy_port {
            api_port = unused_port();
        }
        Self {
            storage: unique_temp_dir("daemon-lifecycle"),
            proxy_port,
            api_port,
        }
    }

    pub(super) fn run(&self, command: &str) -> Output {
        let proxy_port = self.proxy_port.to_string();
        let api = format!("127.0.0.1:{}", self.api_port);
        command_output(
            Command::new(env!("CARGO_BIN_EXE_rsproxy"))
                .arg(command)
                .args([
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &proxy_port,
                    "--api",
                    &api,
                    "--storage",
                ])
                .arg(&self.storage)
                .args(["--no-mitm", "--trace-disk-budget", "0"]),
        )
    }

    pub(super) fn pid_path(&self) -> PathBuf {
        self.storage.join("run/rsproxy.pid")
    }

    pub(super) fn pid(&self) -> u32 {
        fs::read_to_string(self.pid_path())
            .expect("daemon pidfile should exist")
            .trim()
            .parse()
            .expect("daemon pidfile should contain a process id")
    }
}

impl Drop for DaemonHarness {
    fn drop(&mut self) {
        if self.pid_path().is_file() {
            let _ = self.run("stop");
        }
        if let Ok(pid) = fs::read_to_string(self.pid_path())
            && let Ok(pid) = pid.trim().parse::<u32>()
        {
            force_terminate(pid);
        }
        let _ = fs::remove_dir_all(&self.storage);
    }
}

pub(super) fn status_json(harness: &DaemonHarness) -> Value {
    let output = harness.run("status");
    assert_success("status", &output);
    serde_json::from_slice(&output.stdout).expect("status should be JSON")
}

pub(super) fn command_output(command: &mut Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rsproxy command should start");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("rsproxy command did not exit within 15 seconds");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed: stdout={} stderr={}",
        stdout(output),
        stderr(output)
    );
}

pub(super) fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub(super) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub(super) fn unused_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub(super) fn unique_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("rsproxy-{label}-{}-{nonce}", std::process::id()))
}

pub(super) fn wait_for_exit(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while process_exists(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!process_exists(pid), "process {pid} did not exit");
}

#[cfg(unix)]
pub(super) fn expected_default_unix_socket(storage: &Path) -> PathBuf {
    rsproxy_platform::process::unix_control_socket_path(storage)
}

fn process_exists(pid: u32) -> bool {
    rsproxy_platform::process::process_alive(pid)
}

pub(super) fn force_terminate(pid: u32) {
    let _ = rsproxy_platform::process::force_terminate_process(pid);
}

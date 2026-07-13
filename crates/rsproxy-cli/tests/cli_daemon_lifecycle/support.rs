use serde_json::Value;
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) struct DaemonHarness {
    pub(super) storage: PathBuf,
    proxy_port: u16,
    api_port: u16,
}

impl DaemonHarness {
    pub(super) fn new() -> Self {
        let (proxy_port, api_port) = probe_ports();
        Self {
            storage: unique_temp_dir("daemon-lifecycle"),
            proxy_port,
            api_port,
        }
    }

    /// Starts the daemon, probing fresh ports and retrying when another
    /// process claimed a probed port before the daemon could bind it.
    pub(super) fn start(&mut self) -> Output {
        for _ in 1..START_ATTEMPTS {
            let output = self.run("start");
            if output.status.success() || !lost_probed_port(&self.storage, &output) {
                return output;
            }
            (self.proxy_port, self.api_port) = probe_ports();
        }
        self.run("start")
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
    let capture = unique_temp_dir("daemon-command-output");
    fs::create_dir_all(&capture).expect("command capture directory should be created");
    let stdout_path = capture.join("stdout");
    let stderr_path = capture.join("stderr");
    let stdout_file = fs::File::create(&stdout_path).expect("stdout capture should be created");
    let stderr_file = fs::File::create(&stderr_path).expect("stderr capture should be created");
    let mut child = command
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .expect("rsproxy command should start");
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let status = child.wait().expect("timed-out command should be reaped");
            let output = captured_output(status, &stdout_path, &stderr_path);
            let _ = fs::remove_dir_all(&capture);
            panic!(
                "rsproxy command did not exit within 15 seconds: stdout={} stderr={}",
                stdout(&output),
                stderr(&output)
            );
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = captured_output(status, &stdout_path, &stderr_path);
    let _ = fs::remove_dir_all(capture);
    output
}

fn captured_output(status: ExitStatus, stdout_path: &Path, stderr_path: &Path) -> Output {
    Output {
        status,
        stdout: fs::read(stdout_path).unwrap_or_default(),
        stderr: fs::read(stderr_path).unwrap_or_default(),
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

const START_ATTEMPTS: usize = 5;

fn probe_ports() -> (u16, u16) {
    let proxy_port = unused_port();
    let mut api_port = unused_port();
    while api_port == proxy_port {
        api_port = unused_port();
    }
    (proxy_port, api_port)
}

/// Runs a daemon start command on a freshly probed proxy port, retrying with
/// a new port when the probe was lost to another process. The probe listener
/// is released before the daemon starts, so the reservation is inherently
/// racy under parallel tests.
pub(super) fn start_daemon_on_probed_port(
    storage: &Path,
    start: impl Fn(u16) -> Output,
) -> (u16, Output) {
    for _ in 1..START_ATTEMPTS {
        let port = unused_port();
        let output = start(port);
        if output.status.success() || !lost_probed_port(storage, &output) {
            return (port, output);
        }
    }
    let port = unused_port();
    (port, start(port))
}

fn lost_probed_port(storage: &Path, output: &Output) -> bool {
    stderr(output).contains("exited during start")
        && fs::read_to_string(storage.join("run/rsproxy.log"))
            .is_ok_and(|log| log.contains("Address already in use"))
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

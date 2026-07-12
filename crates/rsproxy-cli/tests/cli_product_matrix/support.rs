use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct ExpectedResponse {
    pub method: &'static str,
    pub path: &'static str,
    pub body: &'static str,
}

pub(super) fn run(storage: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rsproxy"));
    command
        .args(args)
        .args(["--storage", storage.to_str().unwrap()])
        .env("RSPROXY_HOME", storage)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(stdin) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

pub(super) fn run_offline(storage: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut args = args.to_vec();
    args.extend(["--api", "127.0.0.1:1"]);
    run(storage, &args, stdin)
}

pub(super) fn run_online(
    storage: &Path,
    args: &[&str],
    responses: Vec<ExpectedResponse>,
) -> Output {
    let (api, server) = serve(responses);
    let mut args = args.to_vec();
    args.extend(["--api", &api]);
    let output = run(storage, &args, None);
    server.join().unwrap();
    output
}

pub(super) fn assert_success(label: &str, output: &Output) -> String {
    assert!(
        output.status.success(),
        "{label}: stdout={} stderr={}",
        stdout(output),
        stderr(output)
    );
    stdout(output)
}

pub(super) fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub(super) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub(super) fn unique_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("rsproxy-{label}-{}-{nonce}", std::process::id()))
}

fn serve(responses: Vec<ExpectedResponse>) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let api = listener.local_addr().unwrap().to_string();
    let server = std::thread::spawn(move || {
        for expected in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let wanted = format!("{} {} HTTP/1.1", expected.method, expected.path);
            assert!(
                request_line.starts_with(&wanted),
                "expected {wanted:?}, got {request_line:?}"
            );
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        expected.body.len(),
                        expected.body
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });
    (api, server)
}

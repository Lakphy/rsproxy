#![cfg(unix)]

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[path = "large_stream_resource/support.rs"]
mod support;

use support::{Daemon, RssMonitor, TempStorage, resident_kib, rsproxy_binary};

const ONE_GIB: u64 = 1024 * 1024 * 1024;
const ORIGIN_CHUNK_SIZE: usize = 64 * 1024;

#[test]
#[ignore = "moves 1GiB through a release proxy and samples process RSS"]
fn one_gib_proxy_stream_has_bounded_rss_and_exact_trace() {
    let transfer_bytes = env_u64("RSPROXY_LARGE_STREAM_BYTES", ONE_GIB);
    let max_growth_kib = env_u64("RSPROXY_LARGE_STREAM_MAX_RSS_GROWTH_MB", 96) * 1024;
    assert!(transfer_bytes >= ORIGIN_CHUNK_SIZE as u64);

    let storage = TempStorage::new();
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_addr = origin.local_addr().unwrap();
    let origin_worker = thread::spawn(move || serve_large_response(origin, transfer_bytes));
    let (proxy_addr, api_addr) = reserve_addresses();
    let mut daemon = Daemon::spawn(storage.path(), proxy_addr, api_addr);
    daemon.wait_until_ready(proxy_addr);

    let baseline_kib = resident_kib(daemon.id()).expect("proxy RSS should be observable");
    let monitor = RssMonitor::start(daemon.id(), baseline_kib);
    let started = Instant::now();
    let received = fetch_through_proxy(proxy_addr, origin_addr);
    let elapsed = started.elapsed();
    let max_rss_kib = monitor.stop();
    let origin_sent = origin_worker.join().unwrap().unwrap();

    assert_eq!(origin_sent, transfer_bytes);
    assert_eq!(received, transfer_bytes);
    let growth_kib = max_rss_kib.saturating_sub(baseline_kib);
    assert!(
        growth_kib <= max_growth_kib,
        "proxy RSS grew by {growth_kib}KiB (baseline={baseline_kib}KiB, peak={max_rss_kib}KiB, limit={max_growth_kib}KiB)"
    );

    let session = wait_for_session(storage.path(), api_addr, 1);
    assert_eq!(session["response_bytes"].as_u64(), Some(transfer_bytes));
    let preview = session["res_body_head"]
        .as_str()
        .expect("trace response preview should be text");
    assert_eq!(preview.len(), 4096);
    assert!(preview.bytes().all(|byte| byte == b'x'));
    let stats = trace_json(storage.path(), api_addr, &["trace", "stats"]);
    assert_eq!(stats["queue_dropped"].as_u64(), Some(0));
    assert_eq!(stats["incomplete_sessions"].as_u64(), Some(0));
    let total_memory = stats["total_memory_bytes"]
        .as_u64()
        .expect("trace stats should expose total memory");
    let memory_budget = stats["memory_budget_bytes"]
        .as_u64()
        .expect("trace stats should expose the memory budget");
    assert!(total_memory <= memory_budget);

    println!(
        "large_stream_bytes={transfer_bytes} elapsed_ms={} baseline_rss_kib={baseline_kib} peak_rss_kib={max_rss_kib} growth_rss_kib={growth_kib}",
        elapsed.as_millis()
    );
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn reserve_addresses() -> (SocketAddr, SocketAddr) {
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let api = TcpListener::bind("127.0.0.1:0").unwrap();
    let addresses = (proxy.local_addr().unwrap(), api.local_addr().unwrap());
    drop((proxy, api));
    addresses
}

fn serve_large_response(listener: TcpListener, total: u64) -> io::Result<u64> {
    let (mut stream, _) = listener.accept()?;
    stream.set_nodelay(true)?;
    read_http_head(&mut stream)?;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
    )?;
    let chunk = vec![b'x'; ORIGIN_CHUNK_SIZE];
    let mut sent = 0u64;
    while sent < total {
        let size = (total - sent).min(chunk.len() as u64) as usize;
        stream.write_all(&chunk[..size])?;
        sent += size as u64;
    }
    stream.flush()?;
    Ok(sent)
}

fn read_http_head(stream: &mut TcpStream) -> io::Result<()> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 4096];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended before its header",
            ));
        }
        head.extend_from_slice(&buffer[..read]);
        if head.len() > 64 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request header exceeded acceptance-test limit",
            ));
        }
    }
    Ok(())
}

fn fetch_through_proxy(proxy: SocketAddr, origin: SocketAddr) -> u64 {
    let mut stream = TcpStream::connect(proxy).unwrap();
    stream.set_nodelay(true).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(120)))
        .unwrap();
    write!(
        stream,
        "GET http://{origin}/one-gib HTTP/1.1\r\nHost: {origin}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    stream.flush().unwrap();

    let mut reader = BufReader::with_capacity(128 * 1024, stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(
        line.starts_with("HTTP/1.1 200 "),
        "unexpected status: {line}"
    );
    let mut chunked = false;
    let mut content_length = None;
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = Some(value.trim().parse::<u64>().unwrap());
        }
    }
    assert_ne!(
        chunked,
        content_length.is_some(),
        "proxy response should use exactly one supported body framing"
    );
    match content_length {
        Some(length) => read_fixed_body(&mut reader, length),
        None => read_chunked_body(&mut reader),
    }
}

fn read_fixed_body(reader: &mut impl Read, length: u64) -> u64 {
    let mut remaining = length;
    let mut buffer = [0u8; ORIGIN_CHUNK_SIZE];
    while remaining > 0 {
        let limit = remaining.min(buffer.len() as u64) as usize;
        reader.read_exact(&mut buffer[..limit]).unwrap();
        assert!(buffer[..limit].iter().all(|byte| *byte == b'x'));
        remaining -= limit as u64;
    }
    length
}

fn read_chunked_body(reader: &mut impl BufRead) -> u64 {
    let mut total = 0u64;
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        let size = usize::from_str_radix(line.trim().split(';').next().unwrap(), 16).unwrap();
        if size == 0 {
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    return total;
                }
            }
        }
        assert!(size <= 1024 * 1024, "unexpected proxy chunk size: {size}");
        let mut data = vec![0u8; size];
        reader.read_exact(&mut data).unwrap();
        assert_eq!(data.first(), Some(&b'x'));
        assert_eq!(data.last(), Some(&b'x'));
        let mut terminator = [0u8; 2];
        reader.read_exact(&mut terminator).unwrap();
        assert_eq!(&terminator, b"\r\n");
        total = total.saturating_add(size as u64);
    }
}

fn wait_for_session(storage: &Path, api: SocketAddr, id: u64) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let output = trace_output(storage, api, &["trace", "get", &id.to_string()]);
        if output.status.success() {
            return serde_json::from_slice(&output.stdout).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "session {id} was not published: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn trace_json(storage: &Path, api: SocketAddr, args: &[&str]) -> serde_json::Value {
    let output = trace_output(storage, api, args);
    assert!(
        output.status.success(),
        "trace command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn trace_output(storage: &Path, api: SocketAddr, args: &[&str]) -> std::process::Output {
    Command::new(rsproxy_binary())
        .args(args)
        .args(["--api", &api.to_string(), "--storage"])
        .arg(storage)
        .output()
        .unwrap()
}

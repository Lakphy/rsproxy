use serde::Serialize;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Instant;

#[derive(Clone)]
struct Config {
    proxy: SocketAddr,
    target: String,
    authority: String,
    requests: usize,
    concurrency: usize,
}

#[derive(Default)]
struct WorkerStats {
    latencies_us: Vec<u64>,
    response_bytes: u64,
    status_errors: usize,
    io_errors: usize,
    first_io_error: Option<String>,
}

#[derive(Serialize)]
struct Report<'a> {
    schema: &'static str,
    driver: &'static str,
    proxy: String,
    target: &'a str,
    requests: usize,
    completed_requests: usize,
    concurrency: usize,
    elapsed_ms: u64,
    requests_per_second: f64,
    p50_us: u64,
    p99_us: u64,
    max_us: u64,
    response_bytes: u64,
    status_errors: usize,
    io_errors: usize,
    first_io_error: Option<&'a str>,
}

fn main() {
    match Config::parse().and_then(run) {
        Ok((json, failed)) => {
            println!("{json}");
            if failed {
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("benchmark client failed: {error}");
            std::process::exit(1);
        }
    }
}

impl Config {
    fn parse() -> Result<Self, String> {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        let proxy = option(&args, "--proxy")?
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid --proxy: {error}"))?;
        let target = option(&args, "--target")?;
        let authority = target
            .strip_prefix("http://")
            .ok_or_else(|| "--target must use http://".to_string())?
            .split('/')
            .next()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "--target must include an authority".to_string())?
            .to_string();
        let requests = parse_positive(&option(&args, "--requests")?, "--requests")?;
        let concurrency =
            parse_positive(&option(&args, "--concurrency")?, "--concurrency")?.min(requests);
        Ok(Self {
            proxy,
            target,
            authority,
            requests,
            concurrency,
        })
    }
}

fn run(config: Config) -> Result<(String, bool), String> {
    let next = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(config.concurrency + 1));
    let mut workers = Vec::with_capacity(config.concurrency);
    for _ in 0..config.concurrency {
        let config = config.clone();
        let next = next.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || worker(config, next, barrier)));
    }

    barrier.wait();
    let started = Instant::now();
    let mut stats = WorkerStats::default();
    for worker in workers {
        merge_stats(
            &mut stats,
            worker
                .join()
                .map_err(|_| "benchmark worker panicked".to_string())?,
        );
    }
    let elapsed = started.elapsed();
    stats.latencies_us.sort_unstable();
    let completed = stats.latencies_us.len();
    let report = Report {
        schema: "rsproxy-benchmark/v1",
        driver: "rsproxy-rust-h1",
        proxy: config.proxy.to_string(),
        target: &config.target,
        requests: config.requests,
        completed_requests: completed,
        concurrency: config.concurrency,
        elapsed_ms: elapsed.as_millis().min(u64::MAX as u128) as u64,
        requests_per_second: completed as f64 / elapsed.as_secs_f64().max(f64::EPSILON),
        p50_us: percentile(&stats.latencies_us, 50),
        p99_us: percentile(&stats.latencies_us, 99),
        max_us: stats.latencies_us.last().copied().unwrap_or(0),
        response_bytes: stats.response_bytes,
        status_errors: stats.status_errors,
        io_errors: stats.io_errors,
        first_io_error: stats.first_io_error.as_deref(),
    };
    let failed = completed != config.requests || stats.status_errors != 0 || stats.io_errors != 0;
    serde_json::to_string(&report)
        .map(|json| (json, failed))
        .map_err(|error| error.to_string())
}

fn worker(config: Config, next: Arc<AtomicUsize>, barrier: Arc<Barrier>) -> WorkerStats {
    let mut stats = WorkerStats::default();
    let mut connection = None;
    barrier.wait();
    loop {
        let request_index = next.fetch_add(1, Ordering::Relaxed);
        if request_index >= config.requests {
            break;
        }
        let started = Instant::now();
        if connection.is_none() {
            connection = BenchConnection::connect(config.proxy).ok();
        }
        match connection
            .as_mut()
            .ok_or_else(|| io::Error::other("proxy connect failed"))
            .and_then(|connection| connection.request(&config.target, &config.authority))
        {
            Ok((status, bytes)) => {
                stats
                    .latencies_us
                    .push(started.elapsed().as_micros().min(u64::MAX as u128) as u64);
                stats.response_bytes = stats.response_bytes.saturating_add(bytes as u64);
                if status != 200 {
                    stats.status_errors += 1;
                }
            }
            Err(error) => {
                stats.io_errors += 1;
                if stats.first_io_error.is_none() {
                    stats.first_io_error = Some(error.to_string());
                }
                connection = None;
            }
        }
    }
    stats
}

struct BenchConnection {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl BenchConnection {
    fn connect(proxy: SocketAddr) -> io::Result<Self> {
        let stream = TcpStream::connect(proxy)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            writer: stream.try_clone()?,
            reader: BufReader::new(stream),
        })
    }

    fn request(&mut self, target: &str, authority: &str) -> io::Result<(u16, usize)> {
        write!(
            self.writer,
            "GET {target} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: rsproxy-benchmark/1\r\nAccept: */*\r\nConnection: keep-alive\r\n\r\n"
        )?;
        self.writer.flush()?;

        let mut status_line = String::new();
        self.reader.read_line(&mut status_line)?;
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid status line"))?;
        let mut content_length = None;
        let mut chunked = false;
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "response headers ended early",
                ));
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = Some(value.trim().parse::<usize>().map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid content-length")
                    })?);
                }
                if name.eq_ignore_ascii_case("transfer-encoding")
                    && value
                        .split(',')
                        .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
                {
                    chunked = true;
                }
            }
        }
        if chunked {
            return read_chunked_body(&mut self.reader).map(|length| (status, length));
        }
        let length = content_length
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing content-length"))?;
        let mut body = vec![0; length];
        self.reader.read_exact(&mut body)?;
        Ok((status, length))
    }
}

fn option(args: &[String], name: &str) -> Result<String, String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
        .ok_or_else(|| format!("{name} is required"))
}

fn parse_positive(value: &str, name: &str) -> Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be numeric"))?;
    if value == 0 {
        Err(format!("{name} must be greater than zero"))
    } else {
        Ok(value)
    }
}

fn merge_stats(total: &mut WorkerStats, mut worker: WorkerStats) {
    total.latencies_us.append(&mut worker.latencies_us);
    total.response_bytes = total.response_bytes.saturating_add(worker.response_bytes);
    total.status_errors += worker.status_errors;
    total.io_errors += worker.io_errors;
    if total.first_io_error.is_none() {
        total.first_io_error = worker.first_io_error;
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index.min(sorted.len() - 1)]
}

fn read_chunked_body(reader: &mut BufReader<TcpStream>) -> io::Result<usize> {
    let mut total = 0usize;
    loop {
        let mut size_line = String::new();
        if reader.read_line(&mut size_line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "chunk size ended early",
            ));
        }
        let raw_size = size_line
            .trim_end_matches(['\r', '\n'])
            .split(';')
            .next()
            .unwrap_or_default();
        let size = usize::from_str_radix(raw_size, 16)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;
        if size == 0 {
            loop {
                let mut trailer = String::new();
                if reader.read_line(&mut trailer)? == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "chunk trailers ended early",
                    ));
                }
                if trailer == "\r\n" || trailer == "\n" {
                    return Ok(total);
                }
            }
        }

        let mut remaining = size;
        let mut buffer = [0u8; 8192];
        while remaining > 0 {
            let take = remaining.min(buffer.len());
            reader.read_exact(&mut buffer[..take])?;
            remaining -= take;
        }
        let mut delimiter = [0u8; 2];
        reader.read_exact(&mut delimiter)?;
        if delimiter != *b"\r\n" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid chunk delimiter",
            ));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "body too large"))?;
    }
}

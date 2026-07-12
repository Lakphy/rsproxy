use super::*;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Instant;

#[test]
fn parses_dns_server_lists_and_default_ports() {
    let servers = parse_dns_servers(&[
        "1.1.1.1,[2606:4700:4700::1111]:5353".to_string(),
        "1.1.1.1:53".to_string(),
    ])
    .unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0], "1.1.1.1:53".parse().unwrap());
    assert_eq!(servers[1], "[2606:4700:4700::1111]:5353".parse().unwrap());
    assert!(parse_dns_servers(&["dns.example:53".to_string()]).is_err());
}

#[test]
fn literal_targets_bypass_dns() {
    let resolver = DnsResolver::new(&AppConfig::default()).unwrap();
    assert_eq!(
        resolver.resolve_socket_addrs("127.0.0.1:8080").unwrap(),
        vec!["127.0.0.1:8080".parse().unwrap()]
    );
    assert_eq!(resolver.stats().literal_bypasses, 1);
    assert_eq!(resolver.stats().lookups, 0);
}

#[test]
fn dns_deadline_is_absolute_and_classified() {
    let blackhole = UdpSocket::bind("127.0.0.1:0").unwrap();
    let config = AppConfig {
        dns_servers: vec![blackhole.local_addr().unwrap()],
        dns_timeout: Duration::from_millis(40),
        ..AppConfig::default()
    };
    let resolver = DnsResolver::new(&config).unwrap();
    let started = Instant::now();
    let error = resolver
        .resolve_socket_addrs("stall.rsproxy-dns-timeout.internal:80")
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error:?}");
    assert!(
        error
            .to_string()
            .starts_with("stage=dns: timeout after 40ms")
    );
    assert!(started.elapsed() >= Duration::from_millis(30));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(resolver.stats().timeouts, 1);
}

#[test]
fn positive_answers_are_cached_without_requerying_nameserver() {
    assert_positive_cache_without_requery(true);
    assert_positive_cache_without_requery(false);
}

fn assert_positive_cache_without_requery(include_aaaa: bool) {
    let (server, query_count, stop, worker) = start_positive_dns_fixture(include_aaaa);

    let config = AppConfig {
        dns_servers: vec![server],
        dns_timeout: Duration::from_millis(250),
        ..AppConfig::default()
    };
    let resolver = DnsResolver::new(&config).unwrap();
    let host = if include_aaaa {
        "dual-stack-cache.rsproxy-dns.internal:8080"
    } else {
        "ipv4-cache.rsproxy-dns.internal:8080"
    };
    let first = resolver.resolve_socket_addrs(host).unwrap();
    assert!(first.contains(&"127.0.0.1:8080".parse().unwrap()));
    let first_query_count = query_count.load(Ordering::Relaxed);
    assert!(first_query_count >= 1);

    let second = resolver.resolve_socket_addrs(host).unwrap();
    assert_eq!(second, first);
    thread::sleep(Duration::from_millis(30));
    assert_eq!(query_count.load(Ordering::Relaxed), first_query_count);

    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
    assert_eq!(resolver.stats().lookups, 2);
    assert_eq!(resolver.stats().successes, 2);
}

#[test]
fn zero_cache_ttl_disables_response_cache() {
    let (server, query_count, stop, worker) = start_positive_dns_fixture(true);
    let config = AppConfig {
        dns_servers: vec![server],
        dns_timeout: Duration::from_millis(250),
        dns_cache_ttl: Duration::ZERO,
        ..AppConfig::default()
    };
    let resolver = DnsResolver::new(&config).unwrap();

    resolver
        .resolve_socket_addrs("uncached.rsproxy-dns.internal:8080")
        .unwrap();
    let first_query_count = query_count.load(Ordering::Relaxed);
    resolver
        .resolve_socket_addrs("uncached.rsproxy-dns.internal:8080")
        .unwrap();
    thread::sleep(Duration::from_millis(30));
    assert!(query_count.load(Ordering::Relaxed) > first_query_count);

    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
}

fn start_positive_dns_fixture(
    include_aaaa: bool,
) -> (
    SocketAddr,
    Arc<AtomicU64>,
    Arc<AtomicBool>,
    thread::JoinHandle<()>,
) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_millis(25)))
        .unwrap();
    let server = socket.local_addr().unwrap();
    let query_count = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let worker_count = query_count.clone();
    let worker_stop = stop.clone();
    let worker = thread::spawn(move || {
        let mut query = [0u8; 512];
        while !worker_stop.load(Ordering::Relaxed) {
            let Ok((len, peer)) = socket.recv_from(&mut query) else {
                continue;
            };
            worker_count.fetch_add(1, Ordering::Relaxed);
            let response = positive_dns_response(&query[..len], include_aaaa);
            socket.send_to(&response, peer).unwrap();
        }
    });
    (server, query_count, stop, worker)
}

fn positive_dns_response(query: &[u8], include_aaaa: bool) -> Vec<u8> {
    assert!(query.len() >= 17);
    let question_end = dns_question_end(query);
    let query_type = u16::from_be_bytes([query[question_end - 4], query[question_end - 3]]);
    let answer = match query_type {
        1 => Some((1u16, vec![127, 0, 0, 1])),
        28 if include_aaaa => Some((28u16, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])),
        _ => None,
    };
    let authority = if answer.is_none() {
        dns_soa_record(60)
    } else {
        Vec::new()
    };
    let mut response = Vec::with_capacity(query.len() + 32);
    response.extend_from_slice(&query[..2]);
    response.extend_from_slice(&0x8180u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&(answer.is_some() as u16).to_be_bytes());
    response.extend_from_slice(&(answer.is_none() as u16).to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&query[12..question_end]);
    if let Some((record_type, address)) = answer {
        response.extend_from_slice(&0xc00cu16.to_be_bytes());
        response.extend_from_slice(&record_type.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&60u32.to_be_bytes());
        response.extend_from_slice(&(address.len() as u16).to_be_bytes());
        response.extend_from_slice(&address);
    }
    response.extend_from_slice(&authority);
    response
}

fn dns_soa_record(ttl: u32) -> Vec<u8> {
    let mut rdata = Vec::new();
    rdata.extend_from_slice(&0xc00cu16.to_be_bytes());
    rdata.extend_from_slice(&0xc00cu16.to_be_bytes());
    for value in [1u32, 60, 60, 60, ttl] {
        rdata.extend_from_slice(&value.to_be_bytes());
    }
    let mut record = Vec::new();
    record.extend_from_slice(&0xc00cu16.to_be_bytes());
    record.extend_from_slice(&6u16.to_be_bytes());
    record.extend_from_slice(&1u16.to_be_bytes());
    record.extend_from_slice(&ttl.to_be_bytes());
    record.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    record.extend_from_slice(&rdata);
    record
}

fn dns_question_end(query: &[u8]) -> usize {
    let mut position = 12;
    loop {
        let label_len = query[position] as usize;
        position += 1;
        if label_len == 0 {
            break;
        }
        position += label_len;
        assert!(position < query.len());
    }
    position + 4
}

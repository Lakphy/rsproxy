use super::super::support;
use super::*;
use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::sync::mpsc::{self, Receiver};

pub(super) struct OriginReply {
    pub(super) status: u16,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
    pub(super) trailers: Vec<(String, String)>,
}

impl OriginReply {
    pub(super) fn ok(body: impl AsRef<[u8]>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: body.as_ref().to_vec(),
            trailers: Vec::new(),
        }
    }
}

pub(super) struct TestOrigin {
    pub(super) address: SocketAddr,
    request: Receiver<RawRequest>,
    worker: thread::JoinHandle<()>,
}

impl TestOrigin {
    pub(super) fn spawn(reply: OriginReply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, request) = mpsc::channel();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = http::read_request(&mut stream, 128 * 1024, 256)
                .unwrap()
                .expect("origin should receive a request");
            sender.send(request).unwrap();
            if reply.trailers.is_empty() {
                http::write_response(
                    &mut stream,
                    reply.status,
                    http::reason_phrase(reply.status),
                    &reply.headers,
                    &reply.body,
                )
                .unwrap();
            } else {
                write_chunked_origin_response(&mut stream, &reply).unwrap();
            }
        });
        Self {
            address,
            request,
            worker,
        }
    }

    pub(super) fn finish(self) -> RawRequest {
        self.worker.join().unwrap();
        self.request.recv_timeout(Duration::from_secs(1)).unwrap()
    }
}

pub(super) struct Exchange {
    pub(super) head: http::RawResponseHead,
    pub(super) body: ResponseBody,
    pub(super) elapsed: Duration,
}

pub(super) fn state_with_rules(name: &str, rules: &str) -> SharedState {
    let mut state = test_state();
    state.config.storage = std::env::temp_dir().join(format!(
        "rsproxy-action-effects-{name}-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        rsproxy_rules::RuleSet::parse("effects", rules).unwrap(),
    );
    state
}

pub(super) fn cleanup_state(state: &SharedState) {
    let _ = fs::remove_dir_all(&state.config.storage);
}

pub(super) fn run_exchange(
    state: &SharedState,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Exchange {
    let (proxy, worker) = support::spawn_proxy(state.clone(), 1);
    let mut client = support::connect_client(proxy);
    let authority = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split('/').next())
        .unwrap();
    write!(client, "{method} {url} HTTP/1.1\r\nHost: {authority}\r\n").unwrap();
    for (name, value) in headers {
        write!(client, "{name}: {value}\r\n").unwrap();
    }
    if !body.is_empty() {
        write!(client, "Content-Length: {}\r\n", body.len()).unwrap();
    }
    write!(client, "Connection: close\r\n\r\n").unwrap();
    client.write_all(body).unwrap();
    client.flush().unwrap();

    let started = Instant::now();
    let head = http::read_response_head(&mut client, 128 * 1024, 256).unwrap();
    let body = read_response_body(&mut client, &head.headers).unwrap();
    let elapsed = started.elapsed();
    drop(client);
    worker.join().unwrap();
    Exchange {
        head,
        body,
        elapsed,
    }
}

pub(super) fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    http::header(headers, name)
}

fn write_chunked_origin_response(stream: &mut impl Write, reply: &OriginReply) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\n",
        reply.status,
        http::reason_phrase(reply.status)
    )?;
    for (name, value) in &reply.headers {
        if !name.eq_ignore_ascii_case("content-length")
            && !name.eq_ignore_ascii_case("transfer-encoding")
            && !name.eq_ignore_ascii_case("connection")
        {
            write!(stream, "{name}: {value}\r\n")?;
        }
    }
    write!(
        stream,
        "Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
    )?;
    if !reply.body.is_empty() {
        write!(stream, "{:X}\r\n", reply.body.len())?;
        stream.write_all(&reply.body)?;
        write!(stream, "\r\n")?;
    }
    write!(stream, "0\r\n")?;
    for (name, value) in &reply.trailers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.flush()
}

use bytes::Bytes;
use rsproxy_net::{
    ActivityStore, AsyncIo, DnsConfig, DnsResolver, DnsStatsSnapshot, DownstreamH2Config,
    DownstreamH2Request, DownstreamH2Response, DownstreamH2ResponseFrame, DownstreamH2ResponseHead,
    H2Body, H2Config, H2DispatchRequest, H2Outcome, KeyedActivity, NetError, NetResult, NetStage,
    PoolWaitSpec, ProtocolErrorKind, ReadyIo, RequestDeadline, UpstreamBody, UpstreamH2Request,
    acquire_slot, dispatch, header, read_request, serve_downstream_h2,
};
use std::io::{self, Cursor, Read, Write};
use std::net::TcpStream;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};

#[test]
fn typed_error_facade_is_public() {
    fn assert_error<T: std::error::Error + Send + Sync + 'static>() {}
    fn accept_result(_: NetResult<()>) {}

    assert_error::<NetError>();
    accept_result(Err(NetError::Protocol {
        kind: ProtocolErrorKind::MalformedMessage,
        stage: NetStage::Request,
        message: "invalid request line".to_string(),
    }));
}

#[test]
fn public_protocol_api_parses_and_streams_http() {
    let raw = b"POST /items HTTP/1.1\r\nHost: api.example.test\r\nContent-Length: 4\r\n\r\nbody";
    let request = read_request(&mut Cursor::new(raw), 4096, 16)
        .unwrap()
        .unwrap();
    assert_eq!(request.target, "/items");
    assert_eq!(header(&request.headers, "host"), Some("api.example.test"));
    assert_eq!(request.body, b"body");

    fn assert_send<T: Send>() {}
    assert_send::<UpstreamBody>();
}

#[test]
fn public_deadline_and_pool_api_reserve_capacity() {
    let deadline = RequestDeadline::new(Duration::from_secs(1)).unwrap();
    assert!(deadline.remaining().unwrap() <= Duration::from_secs(1));

    let activity = Mutex::new(KeyedActivity::default());
    let available = Condvar::new();
    acquire_slot(
        &activity,
        &available,
        "origin.example.test:443",
        1,
        Duration::from_secs(1),
        Instant::now(),
        PoolWaitSpec {
            stage: "upstream",
            limit_label: "connection limit",
        },
    )
    .unwrap();
    let mut activity = activity.lock().unwrap();
    assert_eq!(activity.active_for("origin.example.test:443"), 1);
    activity.release("origin.example.test:443");
}

#[test]
fn public_upstream_h2_api_exposes_typed_dispatch() {
    let request = H2DispatchRequest {
        pool_key: "https://origin.example.test:443",
        request: UpstreamH2Request {
            method: "GET".to_string(),
            uri: "https://origin.example.test/items".to_string(),
            headers: vec![("accept".to_string(), "application/json".to_string())],
            body: Vec::new(),
            trailers: Vec::new(),
        },
        body: H2Body::Buffered,
        config: H2Config {
            max_header_size: 64 * 1024,
            max_header_count: 128,
            max_active_streams_per_key: 16,
            pool_wait_timeout: Duration::from_secs(1),
            ttfb_timeout: Duration::from_secs(5),
            deadline: RequestDeadline::new(Duration::from_secs(10)).unwrap(),
        },
    };
    assert_eq!(request.body, H2Body::Buffered);
    let _dispatch: for<'a> fn(H2DispatchRequest<'a>) -> io::Result<H2Outcome> = dispatch;
}

#[test]
fn public_async_adapter_accepts_ready_io_implementations() {
    fn assert_async_adapter<T: AsyncRead + AsyncWrite>() {}
    assert_async_adapter::<AsyncIo<TestIo>>();
}

#[test]
fn public_downstream_h2_api_accepts_generic_callbacks() {
    type Handler = fn(DownstreamH2Request) -> std::future::Ready<io::Result<DownstreamH2Response>>;
    let serve: fn(TcpStream, String, DownstreamH2Config, Handler) -> io::Result<()> =
        serve_downstream_h2::<TcpStream, Handler, _>;
    let _ = serve;

    fn handler(
        _request: DownstreamH2Request,
    ) -> std::future::Ready<io::Result<DownstreamH2Response>> {
        let (sender, body) = tokio::sync::mpsc::channel(1);
        sender
            .try_send(Ok(DownstreamH2ResponseFrame::Data(Bytes::from_static(
                b"ok",
            ))))
            .unwrap();
        drop(sender);
        std::future::ready(Ok(DownstreamH2Response {
            head: DownstreamH2ResponseHead {
                status: 200,
                headers: Vec::new(),
            },
            body,
        }))
    }

    let _handler: Handler = handler;
}

#[test]
fn public_dns_api_bypasses_lookup_for_literal_addresses() {
    let resolver = DnsResolver::new(&DnsConfig {
        servers: Vec::new(),
        timeout: Duration::from_secs(1),
        cache_ttl: Duration::from_secs(60),
    })
    .unwrap();
    assert_eq!(
        resolver.resolve_socket_addrs("127.0.0.1:8080").unwrap(),
        vec!["127.0.0.1:8080".parse().unwrap()]
    );
    let stats: DnsStatsSnapshot = resolver.stats();
    assert_eq!(stats.literal_bypasses, 1);
    assert_eq!(stats.lookups, 0);
}

struct TestIo(Cursor<Vec<u8>>);

impl Read for TestIo {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Write for TestIo {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl ReadyIo for TestIo {
    fn set_nonblocking(&mut self, _nonblocking: bool) -> io::Result<()> {
        Ok(())
    }

    fn begin_shutdown(&mut self) {}

    fn shutdown_write(&mut self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(unix)]
    fn raw_fd(&self) -> std::os::fd::RawFd {
        -1
    }
}

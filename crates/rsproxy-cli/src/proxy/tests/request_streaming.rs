use super::support::wait_for_trace_stats;
use super::*;
use std::sync::mpsc::{self, Receiver};

mod chunked;
mod fixed;
mod rules;

type OriginReply = (Vec<(String, String)>, Vec<u8>);

fn spawn_origin(
    requests: usize,
    reply: impl Fn(usize, &RawRequest) -> OriginReply + Send + 'static,
) -> (
    std::net::SocketAddr,
    Receiver<RawRequest>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        for index in 0..requests {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = http::read_request(&mut stream, 64 * 1024, 128)
                .unwrap()
                .unwrap();
            let (headers, body) = reply(index, &request);
            tx.send(request).unwrap();
            http::write_response(&mut stream, 200, "OK", &headers, &body).unwrap();
        }
    });
    (address, rx, worker)
}

fn spawn_proxy(
    state: SharedState,
    clients: usize,
) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = thread::spawn(move || {
        for _ in 0..clients {
            let (stream, _) = listener.accept().unwrap();
            handle_client(stream, state.clone()).unwrap();
        }
    });
    (address, worker)
}

fn connect_client(address: std::net::SocketAddr) -> TcpStream {
    let stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
}

fn read_response(stream: &mut TcpStream) -> (http::RawResponseHead, ResponseBody) {
    let head = http::read_response_head(stream, 64 * 1024, 128).unwrap();
    let body = read_response_body(stream, &head.headers).unwrap();
    (head, body)
}

fn response_header<'a>(head: &'a http::RawResponseHead, name: &str) -> Option<&'a str> {
    http::header(&head.headers, name)
}

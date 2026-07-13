//! Minimal persistent HTTP/1 origin used by repository proxy benchmarks.
//!
//! The program prints its effective listener address once, then serves a fixed
//! 1 KiB body so benchmark clients measure proxy overhead rather than application work.

use bytes::Bytes;
use http_body_util::Full;
use hyper::Response;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::io::{self, Write};
use tokio::net::TcpListener;

const RESPONSE_BYTES: usize = 1024;

fn main() -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run())
}

async fn run() -> io::Result<()> {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:0".to_string());
    let listener = TcpListener::bind(&address).await?;
    println!("origin_addr={}", listener.local_addr()?);
    io::stdout().flush()?;

    let body = Bytes::from(vec![b'x'; RESPONSE_BYTES]);
    loop {
        let (stream, _) = listener.accept().await?;
        stream.set_nodelay(true)?;
        let body = body.clone();
        tokio::spawn(async move {
            let service = service_fn(move |_| {
                let body = body.clone();
                async move {
                    let response = Response::builder()
                        .header("content-type", "application/octet-stream")
                        .body(Full::new(body))
                        .expect("static benchmark response is valid");
                    Ok::<_, Infallible>(response)
                }
            });
            if let Err(error) = http1::Builder::new()
                .keep_alive(true)
                .serve_connection(TokioIo::new(stream), service)
                .await
            {
                eprintln!("benchmark origin connection failed: {error}");
            }
        });
    }
}

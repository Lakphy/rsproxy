use super::{H2Body, h2_runtime};
use crate::app::SharedState;
use crate::async_io::AsyncIo;
use crate::http::{RequestBodyFraming, RequestHead};
use crate::proxy;
use crate::proxy::{H2BridgeIo, H2RequestFrame, H2RequestSender};
use http_body_util::BodyExt;
use hyper::body::Body as _;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rsproxy_trace::TlsRecord;
use rustls::{ServerConnection, StreamOwned};
use std::convert::Infallible;
use std::io;
use std::net::TcpStream;
use std::sync::Arc;

use super::message::{
    H2RequestError, error_response, hyper_response, raw_request_head, request_trailers,
    validate_request_headers,
};

const H2_MAX_CONCURRENT_STREAMS: u32 = 256;
const H2_REQUEST_CHANNEL_CAPACITY: usize = 8;
const H2_HEADER_DIAGNOSTIC_MARGIN: usize = 64 * 1024;

#[derive(Clone)]
struct H2Context {
    state: SharedState,
    peer: String,
    connect_authority: String,
    client_tls: TlsRecord,
    initial_flags: Vec<String>,
}

pub(crate) fn serve_mitm(
    tls: StreamOwned<ServerConnection, TcpStream>,
    state: SharedState,
    peer: String,
    connect_authority: String,
    client_tls: TlsRecord,
    initial_flags: Vec<String>,
) -> io::Result<()> {
    let context = Arc::new(H2Context {
        state,
        peer,
        connect_authority,
        client_tls,
        initial_flags,
    });
    let max_header_size = context
        .state
        .config
        .max_header_size
        .saturating_add(H2_HEADER_DIAGNOSTIC_MARGIN)
        .min(u32::MAX as usize) as u32;
    h2_runtime()?.block_on(async move {
        let service = service_fn(move |request| {
            let context = Arc::clone(&context);
            async move { Ok::<_, Infallible>(handle_request(request, context).await) }
        });
        let mut builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
        builder
            .max_concurrent_streams(H2_MAX_CONCURRENT_STREAMS)
            .max_header_list_size(max_header_size);
        builder
            .serve_connection(TokioIo::new(AsyncIo::new(tls)?), service)
            .await
            .map_err(io::Error::other)
    })
}

async fn handle_request(request: Request<Incoming>, context: Arc<H2Context>) -> Response<H2Body> {
    match bridge_request(request, context).await {
        Ok(response) => response,
        Err(error) => error_response(error.status, &error.message),
    }
}

async fn bridge_request(
    request: Request<Incoming>,
    context: Arc<H2Context>,
) -> Result<Response<H2Body>, H2RequestError> {
    if request.method() == Method::CONNECT {
        return Err(H2RequestError::new(
            StatusCode::NOT_IMPLEMENTED,
            "CONNECT over HTTP/2 is not supported",
        ));
    }
    validate_request_headers(&request, &context.state)?;
    let has_body = !request.body().is_end_stream();
    let (parts, body) = request.into_parts();
    let (request, authority) = raw_request_head(parts, &context.connect_authority)?;
    let method = request.method.clone();
    let framing = if has_body {
        RequestBodyFraming::Chunked
    } else {
        RequestBodyFraming::None
    };
    let head = RequestHead {
        request,
        body: framing,
    };
    let (request_sender, request_receiver) =
        tokio::sync::mpsc::channel(H2_REQUEST_CHANNEL_CAPACITY);
    let (bridge, output) = H2BridgeIo::new(
        request_receiver,
        tokio::runtime::Handle::current(),
        &method,
        context.state.config.max_header_size,
        context.state.config.max_header_count,
    );
    let state = context.state.clone();
    let peer = context.peer.clone();
    let client_tls = context.client_tls.clone();
    let mut flags = context.initial_flags.clone();
    flags.push("h2-client".to_string());
    tokio::task::spawn_blocking(move || {
        let _ = proxy::process_h2_request(head, bridge, state, peer, authority, client_tls, flags);
    });
    if has_body {
        let state = context.state.clone();
        tokio::spawn(pump_request_body(body, request_sender, state));
    } else {
        drop(request_sender);
    }
    let response_head = output
        .head
        .await
        .map_err(|_| {
            H2RequestError::new(
                StatusCode::BAD_GATEWAY,
                "HTTP/2 bridge worker stopped before producing a response",
            )
        })?
        .map_err(|error| H2RequestError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;
    hyper_response(response_head, output.body)
}

async fn pump_request_body(mut body: Incoming, sender: H2RequestSender, state: SharedState) {
    while let Some(frame) = body.frame().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                let _ = sender
                    .send(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("failed to read HTTP/2 request body: {error}"),
                    )))
                    .await;
                return;
            }
        };
        let frame = match frame.into_data() {
            Ok(data) => H2RequestFrame::Data(data),
            Err(frame) => match frame.into_trailers() {
                Ok(trailers) => match request_trailers(&trailers, &state) {
                    Ok(trailers) => H2RequestFrame::Trailers(trailers),
                    Err(error) => {
                        let _ = sender
                            .send(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                error.message,
                            )))
                            .await;
                        return;
                    }
                },
                Err(_) => continue,
            },
        };
        let terminal = matches!(frame, H2RequestFrame::Trailers(_));
        if sender.send(Ok(frame)).await.is_err() || terminal {
            return;
        }
    }
}

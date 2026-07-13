use super::{
    DownstreamH2Body, DownstreamH2Config, DownstreamH2Request, DownstreamH2RequestFrame,
    DownstreamH2Response,
};
use http_body_util::BodyExt;
use hyper::body::Body as _;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::convert::Infallible;
use std::future::Future;
use std::io;
use tokio::sync::mpsc;

use crate::{AsyncIo, ReadyIo, RequestBodyFraming, RequestHead, h2_runtime};

use super::message::{
    DownstreamH2RequestError, error_response, hyper_response, raw_request_head, request_trailers,
    validate_request_headers,
};

const H2_MAX_CONCURRENT_STREAMS: u32 = 256;
const H2_REQUEST_CHANNEL_CAPACITY: usize = 8;
const H2_HEADER_DIAGNOSTIC_MARGIN: usize = 64 * 1024;

pub(super) fn serve<S, H, F>(
    io: S,
    default_authority: String,
    config: DownstreamH2Config,
    handler: H,
) -> io::Result<()>
where
    S: ReadyIo,
    H: Fn(DownstreamH2Request) -> F + Clone + Send + Sync + 'static,
    F: Future<Output = io::Result<DownstreamH2Response>> + Send + 'static,
{
    let max_header_size = config
        .max_header_size
        .saturating_add(H2_HEADER_DIAGNOSTIC_MARGIN)
        .min(u32::MAX as usize) as u32;
    h2_runtime()?.block_on(async move {
        let service = service_fn(move |request| {
            let default_authority = default_authority.clone();
            let handler = handler.clone();
            async move {
                Ok::<_, Infallible>(
                    handle_request(request, default_authority, config, handler).await,
                )
            }
        });
        let mut builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
        builder
            .max_concurrent_streams(H2_MAX_CONCURRENT_STREAMS)
            .max_header_list_size(max_header_size);
        builder
            .serve_connection(TokioIo::new(AsyncIo::new(io)?), service)
            .await
            .map_err(io::Error::other)
    })
}

async fn handle_request<H, F>(
    request: Request<Incoming>,
    default_authority: String,
    config: DownstreamH2Config,
    handler: H,
) -> Response<DownstreamH2Body>
where
    H: Fn(DownstreamH2Request) -> F,
    F: Future<Output = io::Result<DownstreamH2Response>>,
{
    match dispatch_request(request, &default_authority, config, handler).await {
        Ok(response) => response,
        Err(error) => error_response(error.status, &error.message),
    }
}

async fn dispatch_request<H, F>(
    request: Request<Incoming>,
    default_authority: &str,
    config: DownstreamH2Config,
    handler: H,
) -> Result<Response<DownstreamH2Body>, DownstreamH2RequestError>
where
    H: Fn(DownstreamH2Request) -> F,
    F: Future<Output = io::Result<DownstreamH2Response>>,
{
    if request.method() == Method::CONNECT {
        return Err(DownstreamH2RequestError::new(
            StatusCode::NOT_IMPLEMENTED,
            "CONNECT over HTTP/2 is not supported",
        ));
    }
    validate_request_headers(&request, &config)?;
    let has_body = !request.body().is_end_stream();
    let (parts, body) = request.into_parts();
    let (request, authority) = raw_request_head(parts, default_authority)?;
    let framing = if has_body {
        RequestBodyFraming::Chunked
    } else {
        RequestBodyFraming::None
    };
    let head = RequestHead {
        request,
        body: framing,
    };
    let (request_sender, request_receiver) = mpsc::channel(H2_REQUEST_CHANNEL_CAPACITY);
    if has_body {
        tokio::spawn(pump_request_body(body, request_sender, config));
    } else {
        drop(request_sender);
    }
    let response = handler(DownstreamH2Request {
        head,
        authority,
        body: request_receiver,
    })
    .await
    .map_err(|error| DownstreamH2RequestError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;
    hyper_response(response)
}

async fn pump_request_body(
    mut body: Incoming,
    sender: mpsc::Sender<io::Result<DownstreamH2RequestFrame>>,
    config: DownstreamH2Config,
) {
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
            Ok(data) => DownstreamH2RequestFrame::Data(data),
            Err(frame) => match frame.into_trailers() {
                Ok(trailers) => match request_trailers(&trailers, &config) {
                    Ok(trailers) => DownstreamH2RequestFrame::Trailers(trailers),
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
        let terminal = matches!(frame, DownstreamH2RequestFrame::Trailers(_));
        if sender.send(Ok(frame)).await.is_err() || terminal {
            return;
        }
    }
}

use super::*;
use crate::async_io::{AsyncIo, ReadyIo};
use crate::upstream_body::{UpstreamBodyFrame, UpstreamBodySender};
use http_body_util::BodyExt;
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct ConnectedSender {
    pub(super) sender: H2Sender,
    pub(super) lease: H2PoolLease,
    pub(super) pool_entry: (String, u64),
}

pub(super) fn connect_buffered<S: ReadyIo>(
    lease: H2PoolLease,
    stream: S,
    request: UpstreamH2Request,
    config: H2Config,
) -> io::Result<UpstreamH2Response> {
    h2_runtime()?.block_on(async move {
        let connected =
            connect_sender(lease, stream, config.max_header_size, config.deadline).await?;
        match send_on_sender(
            connected.sender,
            request,
            SendContext {
                config,
                reused_connection: false,
                pool_wait_started: None,
                pool_entry: Some(connected.pool_entry),
                lease: connected.lease,
            },
        )
        .await
        {
            SendOutcome::Response(response) => Ok(response),
            SendOutcome::Stale(_) => Err(stage_error("ready", "new HTTP/2 connection closed")),
            SendOutcome::Error(error) => Err(error),
        }
    })
}

pub(super) async fn connect_sender<S: ReadyIo>(
    lease: H2PoolLease,
    stream: S,
    max_header_size: usize,
    deadline: RequestDeadline,
) -> io::Result<ConnectedSender> {
    let pool_key = lease.key.clone();
    let io = TokioIo::new(AsyncIo::new(stream)?);
    let mut builder = hyper::client::conn::http2::Builder::new(TokioExecutor::new());
    builder.max_header_list_size(max_header_size.min(u32::MAX as usize) as u32);
    let handshake_timeout = deadline.remaining()?;
    let (sender, connection) =
        match tokio::time::timeout(handshake_timeout, builder.handshake(io)).await {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => return Err(stage_error("handshake", error)),
            Err(_) => return Err(deadline.timeout_error()),
        };
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    let state = h2_pool();
    state
        .inner
        .lock()
        .expect("HTTP/2 pool lock poisoned")
        .insert(
            pool_key.clone(),
            PoolEntry {
                generation,
                sender: sender.clone(),
                last_used: Instant::now(),
            },
        );
    state.available.notify_all();
    spawn_idle_eviction(pool_key.clone(), generation);
    let connection_key = pool_key.clone();
    tokio::spawn(async move {
        let _ = connection.await;
        remove_pool_entry(&connection_key, generation);
    });
    Ok(ConnectedSender {
        sender,
        lease,
        pool_entry: (pool_key, generation),
    })
}

pub(super) enum SendOutcome {
    Response(UpstreamH2Response),
    Stale(H2PoolLease),
    Error(io::Error),
}

pub(super) struct SendContext {
    pub(super) config: H2Config,
    pub(super) reused_connection: bool,
    pub(super) pool_wait_started: Option<Instant>,
    pub(super) pool_entry: Option<(String, u64)>,
    pub(super) lease: H2PoolLease,
}

pub(super) async fn send_on_sender(
    mut sender: H2Sender,
    request: UpstreamH2Request,
    context: SendContext,
) -> SendOutcome {
    let SendContext {
        config,
        reused_connection,
        pool_wait_started,
        pool_entry,
        lease,
    } = context;
    let request = match hyper_request(request, config.max_header_size, config.max_header_count) {
        Ok(request) => request,
        Err(error) => return SendOutcome::Error(error),
    };
    let ready_timeout = match config.deadline.remaining() {
        Ok(timeout) => timeout,
        Err(error) => return SendOutcome::Error(error),
    };
    match tokio::time::timeout(ready_timeout, sender.ready()).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => return SendOutcome::Stale(lease),
        Err(_) => return SendOutcome::Error(config.deadline.timeout_error()),
    }
    let pool_wait_ms = pool_wait_started
        .map(|started| duration_millis(started.elapsed()))
        .unwrap_or(0);
    let request_send = TransferTimer::start();
    let (parts, body) = request.into_parts();
    let request = hyper::Request::from_parts(parts, timed_body(body, request_send.clone()));
    let ttfb_started = Instant::now();
    let ttfb_budget = match config.deadline.budget(config.ttfb_timeout) {
        Ok(budget) => budget,
        Err(error) => return SendOutcome::Error(error),
    };
    let response =
        match tokio::time::timeout(ttfb_budget.timeout(), sender.send_request(request)).await {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return SendOutcome::Error(stage_error("send", error)),
            Err(_) => return SendOutcome::Error(ttfb_budget.timeout_error(ttfb_timeout_error)),
        };
    let response_head_ms = duration_millis(ttfb_started.elapsed());
    let request_send_ms = request_send.elapsed_or_current_ms().min(response_head_ms);
    let ttfb_ms = response_head_ms.saturating_sub(request_send_ms);
    let lease = Arc::new(lease);
    match stream_response(
        response,
        ResponseContext {
            config,
            reused_connection,
            pool_wait_ms,
            request_send_ms,
            ttfb_ms,
            pool_entry,
            lease,
        },
    ) {
        Ok(response) => SendOutcome::Response(response),
        Err(error) => SendOutcome::Error(error),
    }
}

pub(super) struct ResponseContext {
    pub(super) config: H2Config,
    pub(super) reused_connection: bool,
    pub(super) pool_wait_ms: u64,
    pub(super) request_send_ms: u64,
    pub(super) ttfb_ms: u64,
    pub(super) pool_entry: Option<(String, u64)>,
    pub(super) lease: Arc<H2PoolLease>,
}

pub(super) fn stream_response(
    response: hyper::Response<hyper::body::Incoming>,
    context: ResponseContext,
) -> io::Result<UpstreamH2Response> {
    let ResponseContext {
        config,
        reused_connection,
        pool_wait_ms,
        request_send_ms,
        ttfb_ms,
        pool_entry,
        lease,
    } = context;
    let (status, headers, body) =
        split_response(response, config.max_header_size, config.max_header_count)?;
    let (sender, body_stream, response_receive) = UpstreamBody::timed_channel();
    tokio::spawn(pump_response_body(
        body,
        sender,
        BodyPumpContext {
            config,
            pool_entry,
            lease,
            response_receive,
        },
    ));
    Ok(UpstreamH2Response {
        status,
        headers,
        body: body_stream,
        reused_connection,
        pool_wait_ms,
        request_send_ms,
        ttfb_ms,
    })
}

struct BodyPumpContext {
    config: H2Config,
    pool_entry: Option<(String, u64)>,
    lease: Arc<H2PoolLease>,
    response_receive: TransferTimer,
}

async fn pump_response_body(
    mut body: hyper::body::Incoming,
    sender: UpstreamBodySender,
    context: BodyPumpContext,
) {
    let BodyPumpContext {
        config,
        pool_entry,
        lease: _lease,
        response_receive,
    } = context;
    loop {
        let timeout = match config.deadline.remaining() {
            Ok(timeout) => timeout,
            Err(error) => {
                send_body_error(&sender, error, None).await;
                break;
            }
        };
        let frame = match tokio::time::timeout(timeout, body.frame()).await {
            Ok(Some(Ok(frame))) => frame,
            Ok(Some(Err(error))) => {
                send_body_error(
                    &sender,
                    stage_error("response_body", error),
                    pool_entry.as_ref(),
                )
                .await;
                break;
            }
            Ok(None) => break,
            Err(_) => {
                send_body_error(&sender, config.deadline.timeout_error(), None).await;
                break;
            }
        };
        let event = match frame.into_data() {
            Ok(data) => Some(Ok(UpstreamBodyFrame::Data(data))),
            Err(frame) => match frame.into_trailers() {
                Ok(trailers) => {
                    match response_trailers(
                        &trailers,
                        config.max_header_size,
                        config.max_header_count,
                    ) {
                        Ok(trailers) => Some(Ok(UpstreamBodyFrame::Trailers(trailers))),
                        Err(error) => {
                            send_body_error(&sender, error, pool_entry.as_ref()).await;
                            break;
                        }
                    }
                }
                Err(_) => None,
            },
        };
        if let Some(event) = event
            && sender.send(event).await.is_err()
        {
            break;
        }
    }
    response_receive.finish();
}

async fn send_body_error(
    sender: &UpstreamBodySender,
    error: io::Error,
    pool_entry: Option<&(String, u64)>,
) {
    if let Some((key, generation)) = pool_entry {
        remove_pool_entry(key, *generation);
    }
    let _ = sender.send(Err(error)).await;
}

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

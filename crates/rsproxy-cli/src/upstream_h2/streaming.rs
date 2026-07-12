use super::*;
use crate::proxy::UpstreamStream;
use connection::{connect_sender, stream_response};
use std::sync::Arc;
use tokio::task::JoinHandle;

pub(super) enum H2StreamingDispatch {
    Request(StreamingH2Request),
    Connect(H2PoolLease),
}

pub(crate) struct StreamingH2Request {
    body: Option<H2RequestBodySender>,
    response: Option<JoinHandle<io::Result<UpstreamH2Response>>>,
    request_lease: Option<Arc<H2PoolLease>>,
    request_send: TransferTimer,
}

impl StreamingH2Request {
    pub(crate) fn send_data(&self, data: Bytes, deadline: RequestDeadline) -> io::Result<bool> {
        let Some(body) = self.body.as_ref() else {
            return Ok(false);
        };
        body.send_data(data, deadline)
    }

    pub(crate) fn send_trailers(
        &self,
        trailers: Vec<(String, String)>,
        deadline: RequestDeadline,
    ) -> io::Result<bool> {
        let Some(body) = self.body.as_ref() else {
            return Ok(false);
        };
        body.send_trailers(trailers, deadline)
    }

    pub(crate) fn send_error(
        &self,
        error: &io::Error,
        deadline: RequestDeadline,
    ) -> io::Result<bool> {
        let Some(body) = self.body.as_ref() else {
            return Ok(false);
        };
        body.send_error(error, deadline)
    }

    pub(crate) fn close_body(&mut self) {
        self.body.take();
    }

    pub(crate) fn finish(
        mut self,
        ttfb_timeout: Duration,
        deadline: RequestDeadline,
    ) -> io::Result<UpstreamH2Response> {
        self.close_body();
        let budget = deadline.budget(ttfb_timeout)?;
        let mut response = self
            .response
            .take()
            .ok_or_else(|| stage_error("send", "streaming response task is missing"))?;
        let response_was_ready = response.is_finished();
        let ttfb_started = Instant::now();
        let result = h2_runtime()?
            .block_on(async { tokio::time::timeout(budget.timeout(), &mut response).await });
        self.request_lease.take();
        match result {
            Ok(Ok(Ok(mut response))) => {
                response.request_send_ms = self.request_send.elapsed_or_current_ms();
                response.ttfb_ms = if response_was_ready {
                    0
                } else {
                    duration_millis(ttfb_started.elapsed())
                };
                Ok(response)
            }
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(error)) => Err(stage_error(
                "send",
                format!("response task failed: {error}"),
            )),
            Err(_) => {
                response.abort();
                Err(budget.timeout_error(ttfb_timeout_error))
            }
        }
    }
}

impl Drop for StreamingH2Request {
    fn drop(&mut self) {
        self.body.take();
        if let Some(response) = self.response.take() {
            response.abort();
        }
        self.request_lease.take();
    }
}

enum StartOutcome {
    Request(StreamingH2Request),
    Stale(H2PoolLease),
    Error(io::Error),
}

pub(super) fn dispatch_streaming(
    pool_key: &str,
    request: UpstreamH2Request,
    config: H2Config,
) -> io::Result<H2StreamingDispatch> {
    let started = Instant::now();
    let pool_budget = config.deadline.budget(config.pool_wait_timeout)?;
    let mut lease = Some(
        acquire_lease(
            pool_key,
            config.max_active_streams_per_key,
            pool_budget.timeout(),
            started,
        )
        .map_err(|error| pool_budget.map_timeout(error))?,
    );
    loop {
        let Some(entry) = wait_for_entry_or_connector(
            pool_key,
            lease.as_mut().expect("HTTP/2 pool lease is present"),
            config.max_active_streams_per_key,
            pool_budget.timeout(),
            started,
        )
        .map_err(|error| pool_budget.map_timeout(error))?
        else {
            return Ok(H2StreamingDispatch::Connect(
                lease.take().expect("HTTP/2 pool lease is present"),
            ));
        };
        match h2_runtime()?.block_on(start_on_sender(
            entry.sender,
            request.clone(),
            StartContext {
                config,
                reused_connection: true,
                pool_wait_started: Some(started),
                pool_entry: Some((pool_key.to_string(), entry.generation)),
                lease: lease.take().expect("HTTP/2 pool lease is present"),
            },
        )) {
            StartOutcome::Request(request) => {
                return Ok(H2StreamingDispatch::Request(request));
            }
            StartOutcome::Stale(returned_lease) => {
                lease = Some(returned_lease);
                remove_pool_entry(pool_key, entry.generation);
            }
            StartOutcome::Error(error) => {
                if !is_request_total_timeout(&error) {
                    remove_pool_entry(pool_key, entry.generation);
                }
                return Err(error);
            }
        }
    }
}

pub(super) fn connect_streaming(
    lease: H2PoolLease,
    stream: UpstreamStream,
    request: UpstreamH2Request,
    config: H2Config,
) -> io::Result<StreamingH2Request> {
    h2_runtime()?.block_on(async move {
        let connected =
            connect_sender(lease, stream, config.max_header_size, config.deadline).await?;
        match start_on_sender(
            connected.sender,
            request,
            StartContext {
                config,
                reused_connection: false,
                pool_wait_started: None,
                pool_entry: Some(connected.pool_entry),
                lease: connected.lease,
            },
        )
        .await
        {
            StartOutcome::Request(request) => Ok(request),
            StartOutcome::Stale(_) => Err(stage_error("ready", "new HTTP/2 connection closed")),
            StartOutcome::Error(error) => Err(error),
        }
    })
}

struct StartContext {
    config: H2Config,
    reused_connection: bool,
    pool_wait_started: Option<Instant>,
    pool_entry: Option<(String, u64)>,
    lease: H2PoolLease,
}

async fn start_on_sender(
    mut sender: H2Sender,
    request: UpstreamH2Request,
    context: StartContext,
) -> StartOutcome {
    let StartContext {
        config,
        reused_connection,
        pool_wait_started,
        pool_entry,
        lease,
    } = context;
    let ready_timeout = match config.deadline.remaining() {
        Ok(timeout) => timeout,
        Err(error) => return StartOutcome::Error(error),
    };
    match tokio::time::timeout(ready_timeout, sender.ready()).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => return StartOutcome::Stale(lease),
        Err(_) => return StartOutcome::Error(config.deadline.timeout_error()),
    }
    let pool_wait_ms = pool_wait_started
        .map(|started| duration_millis(started.elapsed()))
        .unwrap_or(0);
    let (body_sender, body) =
        request_body::request_body_channel(config.max_header_size, config.max_header_count);
    let request_send = TransferTimer::start();
    let body = timed_body(body, request_send.clone());
    let request = match hyper_request_with_body(
        request,
        body,
        config.max_header_size,
        config.max_header_count,
    ) {
        Ok(request) => request,
        Err(error) => return StartOutcome::Error(error),
    };
    let response = sender.send_request(request);
    let request_lease = Arc::new(lease);
    let response_lease = Arc::clone(&request_lease);
    let response_task = tokio::spawn(async move {
        let response = match response.await {
            Ok(response) => response,
            Err(error) => {
                remove_entry_if_present(pool_entry.as_ref());
                return Err(stage_error("send", error));
            }
        };
        let result = stream_response(
            response,
            connection::ResponseContext {
                config,
                reused_connection,
                pool_wait_ms,
                request_send_ms: 0,
                ttfb_ms: 0,
                pool_entry: pool_entry.clone(),
                lease: response_lease,
            },
        );
        if result.is_err() {
            remove_entry_if_present(pool_entry.as_ref());
        }
        result
    });
    StartOutcome::Request(StreamingH2Request {
        body: Some(body_sender),
        response: Some(response_task),
        request_lease: Some(request_lease),
        request_send,
    })
}

fn remove_entry_if_present(pool_entry: Option<&(String, u64)>) {
    if let Some((key, generation)) = pool_entry {
        remove_pool_entry(key, *generation);
    }
}

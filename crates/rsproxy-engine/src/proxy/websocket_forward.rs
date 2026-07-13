use super::*;

pub(in crate::proxy) struct RequestObservation {
    pub bytes: u64,
    pub body_head: Option<Vec<u8>>,
    pub trailers: Option<Vec<(String, String)>>,
    pub flags: Vec<String>,
}

pub(in crate::proxy) struct WebSocketUpgrade {
    pub head: http::RawResponseHead,
    pub matched_rules: Vec<MatchedRule>,
    pub actions: Vec<ResolvedAction>,
    pub request_send_ms: u64,
    pub request: RequestObservation,
}

pub(in crate::proxy) fn finish<W: WsIo + Send>(
    client: &mut W,
    ctx: &ForwardCtx<'_>,
    plain_client_clone: Option<TcpStream>,
    mut upstream: UpstreamStream,
    mut upgrade: WebSocketUpgrade,
) -> io::Result<ForwardResult> {
    let mut response_headers = upgrade.head.headers.clone();
    apply_response_actions(
        &mut upgrade.head,
        &mut response_headers,
        &mut Vec::new(),
        ctx.meta,
        &upgrade.actions,
        ctx.state,
    )?;
    for item in &upgrade.actions {
        if let Action::Delay {
            phase: Phase::Res,
            millis,
        } = item.action
        {
            ctx.deadline.sleep(Duration::from_millis(millis))?;
        }
    }
    restore_upstream_timeouts(&mut upstream)?;
    write_upgrade_response_head(client, &upgrade.head, &response_headers)?;
    let (request_bytes, response_bytes, frames) = websocket_tunnel(
        client,
        plain_client_clone,
        &mut upstream,
        ctx.state.config.trace_body_limit,
    )?;
    Ok(ForwardResult {
        status: upgrade.head.status,
        upstream: ctx.upstream_addr(),
        request_bytes: request_bytes.saturating_add(upgrade.request.bytes),
        request_body_head: upgrade.request.body_head,
        request_trailers: upgrade.request.trailers,
        response_bytes,
        res_headers: response_headers,
        res_trailers: Vec::new(),
        body_head: Vec::new(),
        frames,
        kind: Some(SessionKind::WebSocket),
        response_matched_rules: upgrade.matched_rules,
        response_actions: upgrade.actions,
        protocol: UpstreamProtocol::Http1,
        client_connection: ClientPersistence::Close,
        pool_wait_ms: 0,
        request_send_ms: Some(upgrade.request_send_ms),
        response_receive_ms: None,
        flags: upgrade.request.flags,
        error: None,
    })
}

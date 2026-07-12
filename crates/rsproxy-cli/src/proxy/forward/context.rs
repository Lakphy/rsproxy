use super::*;

pub(in crate::proxy) struct ForwardInput<'a> {
    pub request: &'a RawRequest,
    pub full_url: &'a str,
    pub meta: &'a RequestMeta,
    pub actions: &'a [ResolvedAction],
    pub state: &'a SharedState,
    pub trace_id: u64,
    pub rules: &'a RuleSet,
    pub plain_client_clone: Option<TcpStream>,
    pub client_connection: ClientPersistence,
    pub deadline: RequestDeadline,
    pub request_body: Option<StreamingRequestBody>,
    pub request_body_rules_skipped: bool,
}

pub(in crate::proxy) struct ForwardCtx<'a> {
    pub request: &'a RawRequest,
    pub full_url: &'a str,
    pub url: &'a UrlParts,
    pub meta: &'a RequestMeta,
    pub actions: &'a [ResolvedAction],
    pub state: &'a SharedState,
    pub trace_id: u64,
    pub rules: &'a RuleSet,
    pub route: &'a UpstreamRoute,
    pub headers: &'a [(String, String)],
    pub client_connection: ClientPersistence,
    pub deadline: RequestDeadline,
    pub request_body_rules_skipped: bool,
}

impl ForwardCtx<'_> {
    pub fn upstream_addr(&self) -> String {
        self.route.session_label()
    }

    pub fn websocket_request(&self) -> bool {
        is_websocket_request(&self.request.headers)
    }
}

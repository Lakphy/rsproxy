use super::*;

pub(super) struct SessionInput<'a> {
    pub req: &'a RawRequest,
    pub full_url: &'a str,
    pub peer: String,
    pub initial_tls: Vec<TlsRecord>,
    pub started_ms_override: Option<u64>,
    pub initial_flags: Vec<String>,
    pub resolved: &'a rsproxy_rules::ResolveResult,
    pub meta: &'a RequestMeta,
    pub state: &'a SharedState,
    pub is_mitm: bool,
}

pub(super) fn begin(input: SessionInput<'_>) -> (Session, bool) {
    let SessionInput {
        req,
        full_url,
        peer,
        initial_tls,
        started_ms_override,
        initial_flags,
        resolved,
        meta,
        state,
        is_mitm,
    } = input;
    let mut session = Session::new(
        SessionKind::Http,
        req.method.clone(),
        full_url.to_string(),
        peer,
    );
    if let Some(started_ms) = started_ms_override {
        session.started_ms = started_ms;
    }
    session.flags.extend(initial_flags);
    session.tls = initial_tls;
    session.req_headers = req.headers.clone();
    session.req_trailers = req.trailers.clone();
    session.request_bytes = req.body.len() as u64;
    session.matched_rules = resolved.matched_rules.clone();
    apply_trace_tags(&mut session, &resolved.actions, meta, state);
    let hidden = trace_hidden(&resolved.actions);

    if is_mitm {
        session.flags.push("mitm".to_string());
    }
    if !req.trailers.is_empty() {
        session.flags.push("req-trailers".to_string());
    }
    add_action_flag(&mut session, &resolved.actions, "res-merge", |action| {
        matches!(action, Action::ResMerge(_))
    });
    add_action_flag(&mut session, &resolved.actions, "res-cors", |action| {
        matches!(action, Action::ResCors(_))
    });
    add_action_flag(&mut session, &resolved.actions, "req-cookie", |action| {
        matches!(action, Action::ReqCookie(_))
    });
    add_action_flag(&mut session, &resolved.actions, "res-cookie", |action| {
        matches!(action, Action::ResCookie(_))
    });
    add_action_flag(&mut session, &resolved.actions, "cache", |action| {
        matches!(action, Action::Cache(_))
    });
    add_action_flag(&mut session, &resolved.actions, "res-trailer", |action| {
        matches!(action, Action::ResTrailer(_))
    });
    if !hidden {
        session.id = state
            .trace
            .start(rsproxy_trace::SessionStart::from_session(&session));
    }
    (session, hidden)
}

fn add_action_flag(
    session: &mut Session,
    actions: &[ResolvedAction],
    flag: &str,
    matches: impl Fn(&Action) -> bool,
) {
    if actions.iter().any(|item| matches(&item.action)) {
        session.flags.push(flag.to_string());
    }
}

use super::*;

impl Action {
    pub const FAMILIES: &'static [&'static str] = &[
        "host",
        "upstream",
        "direct",
        "mock",
        "status",
        "redirect",
        "req.header",
        "res.header",
        "res.status",
        "req.method",
        "req.cookie",
        "res.cookie",
        "req.ua",
        "req.referer",
        "req.auth",
        "req.forwarded",
        "req.type",
        "req.charset",
        "res.cors",
        "res.type",
        "res.charset",
        "res.merge",
        "res.trailer",
        "attachment",
        "cache",
        "tls",
        "url.rewrite",
        "url.query",
        "delete",
        "req.body.set",
        "req.body.prepend",
        "req.body.append",
        "req.body.replace",
        "res.body.set",
        "res.body.prepend",
        "res.body.append",
        "res.body.replace",
        "inject",
        "delay.req",
        "delay.res",
        "throttle.req",
        "throttle.res",
        "bypass",
        "hide",
        "tag",
        "skip",
    ];

    pub fn family(&self) -> &'static str {
        match self {
            Action::Host(_) => "host",
            Action::Upstream(_) => "upstream",
            Action::Direct => "direct",
            Action::Mock(_) | Action::MockRaw(_) => "mock",
            Action::Status(_) => "status",
            Action::Redirect { .. } => "redirect",
            Action::ReqHeader(_) => "req.header",
            Action::ResHeader(_) => "res.header",
            Action::ResStatus(_) => "res.status",
            Action::ReqMethod(_) => "req.method",
            Action::ReqCookie(_) => "req.cookie",
            Action::ResCookie(_) => "res.cookie",
            Action::ReqUa(_) => "req.ua",
            Action::ReqReferer(_) => "req.referer",
            Action::ReqAuth(_) => "req.auth",
            Action::ReqForwarded(_) => "req.forwarded",
            Action::ReqType(_) => "req.type",
            Action::ReqCharset(_) => "req.charset",
            Action::ResCors(_) => "res.cors",
            Action::ResType(_) => "res.type",
            Action::ResCharset(_) => "res.charset",
            Action::ResMerge(_) => "res.merge",
            Action::ResTrailer(_) => "res.trailer",
            Action::Attachment(_) => "attachment",
            Action::Cache(_) => "cache",
            Action::Tls(_) => "tls",
            Action::UrlRewrite { .. } => "url.rewrite",
            Action::UrlQuery(_) => "url.query",
            Action::Delete(_) => "delete",
            Action::ReqBody(BodyOp::Set(_)) => "req.body.set",
            Action::ReqBody(BodyOp::Prepend(_)) => "req.body.prepend",
            Action::ReqBody(BodyOp::Append(_)) => "req.body.append",
            Action::ReqBody(BodyOp::Replace { .. }) => "req.body.replace",
            Action::ResBody(BodyOp::Set(_)) => "res.body.set",
            Action::ResBody(BodyOp::Prepend(_)) => "res.body.prepend",
            Action::ResBody(BodyOp::Append(_)) => "res.body.append",
            Action::ResBody(BodyOp::Replace { .. }) => "res.body.replace",
            Action::Inject(_) => "inject",
            Action::Delay {
                phase: Phase::Req, ..
            } => "delay.req",
            Action::Delay {
                phase: Phase::Res, ..
            } => "delay.res",
            Action::Throttle {
                phase: Phase::Req, ..
            } => "throttle.req",
            Action::Throttle {
                phase: Phase::Res, ..
            } => "throttle.res",
            Action::Bypass => "bypass",
            Action::Hide => "hide",
            Action::Tag(_) => "tag",
            Action::Skip(_) => "skip",
        }
    }

    pub(crate) fn is_single(&self) -> bool {
        !matches!(
            self,
            Action::ReqHeader(_)
                | Action::ResHeader(_)
                | Action::ReqCookie(_)
                | Action::ResCookie(_)
                | Action::ResMerge(_)
                | Action::ResTrailer(_)
                | Action::UrlQuery(_)
                | Action::Delete(_)
                | Action::ReqBody(_)
                | Action::ResBody(_)
                | Action::Inject(_)
                | Action::Tag(_)
                | Action::Skip(_)
        )
    }
}

impl InjectTarget {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Js => "js",
            Self::Css => "css",
        }
    }
}

impl InjectMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Prepend => "prepend",
            Self::Replace => "replace",
        }
    }
}

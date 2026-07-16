mod delete;
mod host_pool;
mod replace_pattern;
mod template_validation;
mod value;

pub use delete::{DeleteBodyPath, DeleteBodyPathSegment, DeletePathSegment};
pub use host_pool::HostPool;
pub use replace_pattern::RegexReplacePattern;
pub use value::{Value, valid_value_key};

#[derive(Clone, Debug, PartialEq, Eq)]
/// A validated operation that the proxy applies after a rule matches.
///
/// Variants retain structured [`Value`] sources. Resolving a rule chooses and
/// orders actions, but loading files and mutating protocol messages remains the
/// caller's responsibility.
pub enum Action {
    /// Selects one origin address from a per-rule round-robin pool while preserving `Host`.
    Host(HostPool),
    /// Routes through the proxy or mixed proxy chain described by the value.
    Upstream(Value),
    /// Forces origin routing and overrides a previously matched upstream route.
    Direct,
    /// Short-circuits the request with an inline, referenced, or file-backed body.
    Mock(Value),
    /// Short-circuits with a raw HTTP status line, headers, and body payload.
    MockRaw(Value),
    /// Short-circuits with a structured status/header/body combination.
    MockInline(MockInlineOp),
    /// Short-circuits with the given HTTP response status.
    Status(u16),
    /// Short-circuits with an HTTP redirect.
    Redirect {
        /// Value rendered as the response `Location` header.
        url: Value,
        /// HTTP redirect status selected by the rule.
        code: u16,
    },
    /// Applies a set, removal, or regex replacement to request headers.
    ReqHeader(HeaderOp),
    /// Applies a set, removal, or regex replacement to response headers.
    ResHeader(HeaderOp),
    /// Replaces the upstream response status before downstream forwarding.
    ResStatus(u16),
    /// Replaces the request method before upstream forwarding.
    ReqMethod(Value),
    /// Sets or removes entries in the request `Cookie` header.
    ReqCookie(CookieOp),
    /// Adds or removes response `Set-Cookie` headers.
    ResCookie(CookieOp),
    /// Sets the request `User-Agent` header.
    ReqUa(Value),
    /// Sets the request `Referer` header.
    ReqReferer(Value),
    /// Sets Basic credentials in the request `Authorization` header.
    ReqAuth(Value),
    /// Sets `X-Forwarded-For`, normalizing socket-address values to their IP.
    ReqForwarded(Value),
    /// Sets the media type portion of the request `Content-Type` header.
    ReqType(Value),
    /// Sets the charset parameter of the request `Content-Type` header.
    ReqCharset(Value),
    /// Materializes common CORS response headers from a structured policy.
    ResCors(CorsOp),
    /// Sets the media type portion of the response `Content-Type` header.
    ResType(Value),
    /// Sets the charset parameter of the response `Content-Type` header.
    ResCharset(Value),
    /// Deep-merges an object into an object-valued JSON response.
    ResMerge(Value),
    /// Sets or removes an HTTP/1.1 response trailer.
    ResTrailer(HeaderOp),
    /// Sets `Content-Disposition: attachment`, optionally with a filename.
    Attachment(Option<Value>),
    /// Writes response cache headers, including the explicit no-cache form.
    Cache(CacheOp),
    /// Constrains origin TLS or supplies a client identity for origin mTLS.
    Tls(TlsOp),
    /// Transparently replaces the request origin (scheme, host, port, and
    /// optionally path/query) before forwarding, without a client-visible
    /// redirect. The Whistle `pattern target` / Charles Map Remote equivalent.
    MapRemote(Value),
    /// Rewrites the request path and query before forwarding.
    UrlRewrite {
        /// Plain or regular-expression pattern matched against path and query.
        from: UrlRewritePattern,
        /// Replacement value; regex captures remain available to regex replacement.
        to: Value,
    },
    /// Applies ordered additions, updates, and removals to query parameters.
    UrlQuery(Vec<QueryOp>),
    /// Applies typed Whistle-compatible deletions to request or response properties.
    Delete(Vec<DeleteOp>),
    /// Rewrites a buffered request body and updates its framing metadata.
    ReqBody(BodyOp),
    /// Rewrites a buffered response body and updates its framing metadata.
    ResBody(BodyOp),
    /// Injects bytes into a response selected by its content type.
    Inject(InjectOp),
    /// Delays request or response forwarding.
    Delay {
        /// Forwarding phase at which the delay is observed.
        phase: Phase,
        /// Delay duration in milliseconds.
        millis: u64,
    },
    /// Paces request or response body writes under the request deadline.
    Throttle {
        /// Body direction whose writes are paced.
        phase: Phase,
        /// Maximum sustained transfer rate in bytes per second.
        bytes_per_sec: u64,
    },
    /// Keeps matching `CONNECT` traffic as passthrough instead of enabling MITM.
    Bypass,
    /// Suppresses trace recording without suppressing other matched actions.
    Hide,
    /// Adds a rendered `tag:<value>` flag to the trace.
    Tag(Value),
    /// Suppresses later action families, or all later actions when the list is empty.
    Skip(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A mutation applied to every value of one request header, response header, or trailer.
pub enum HeaderOp {
    /// Replaces existing values or inserts the named field.
    Set {
        /// Case-insensitive HTTP field name.
        name: String,
        /// Value rendered before the field is written.
        value: Value,
    },
    /// Removes every occurrence of the named field.
    Remove {
        /// Case-insensitive HTTP field name to remove.
        name: String,
    },
    /// Replaces every regex match in every value of the named field.
    Replace {
        /// Case-insensitive HTTP field name whose values are transformed.
        name: String,
        /// Regex compiled while the rule model is constructed.
        pattern: RegexReplacePattern,
        /// Rust-regex replacement text, including `$1` and `${name}` captures.
        replacement: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// An ordered query-string mutation.
pub enum QueryOp {
    /// Adds a parameter or replaces its existing value.
    Set {
        /// Decoded parameter name used to identify existing entries.
        name: String,
        /// Value rendered before query serialization.
        value: Value,
    },
    /// Removes all occurrences of a parameter.
    Remove {
        /// Decoded parameter name to remove.
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A typed target of the DSL's `delete(...)` action.
///
/// Whole-body and nested-body variants are conservative: incompatible content
/// types, invalid encodings, missing paths, or over-limit streams leave the body
/// unchanged while independent deletions may still proceed.
pub enum DeleteOp {
    /// Clears the complete URL pathname.
    Pathname,
    /// Removes an indexed pathname segment using original segment positions.
    PathSegment(DeletePathSegment),
    /// Removes every query parameter.
    UrlParams,
    /// Removes every occurrence of the named query parameter.
    UrlParam(String),
    /// Removes every request header with the given case-insensitive name.
    ReqHeader(String),
    /// Removes every response header with the given case-insensitive name.
    ResHeader(String),
    /// Removes the complete request body.
    ReqBody,
    /// Removes the complete response body.
    ResBody,
    /// Deletes a nested field from a supported request JSON or form body.
    ReqBodyPath(DeleteBodyPath),
    /// Deletes a nested field from a supported response JSON or JSONP body.
    ResBodyPath(DeleteBodyPath),
    /// Removes the media type portion of request `Content-Type`.
    ReqType,
    /// Removes the media type portion of response `Content-Type`.
    ResType,
    /// Removes the charset parameter from request `Content-Type`.
    ReqCharset,
    /// Removes the charset parameter from response `Content-Type`.
    ResCharset,
    /// Removes the named request cookie.
    ReqCookie(String),
    /// Removes response `Set-Cookie` fields for the named cookie.
    ResCookie(String),
    /// Removes the entire request `Cookie` header.
    ReqCookies,
    /// Removes every response `Set-Cookie` header.
    ResCookies,
    /// Removes the named response trailer.
    Trailer(String),
    /// Removes every response trailer.
    Trailers,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A request-cookie or response-cookie mutation.
pub enum CookieOp {
    /// Writes the named cookie, with attributes used for response cookies.
    Set {
        /// Cookie name.
        name: String,
        /// Cookie value rendered at action execution time.
        value: Value,
        /// Ordered `Set-Cookie` attributes; ignored for request cookies.
        attrs: Vec<CookieAttr>,
    },
    /// Removes all entries with the named cookie key.
    Remove {
        /// Cookie name matched case-sensitively according to cookie syntax.
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// One parsed `Set-Cookie` attribute attached to [`CookieOp::Set`].
pub struct CookieAttr {
    /// Attribute name, such as `Path`, `HttpOnly`, or `SameSite`.
    pub name: String,
    /// Optional attribute value; flag attributes such as `Secure` have none.
    pub value: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Structured payload of the inline `mock(status=..., header=..., body=...)` form.
pub struct MockInlineOp {
    /// Response status; `None` defaults to 200.
    pub status: Option<u16>,
    /// Ordered response headers set on the mock response.
    pub headers: Vec<(String, Value)>,
    /// Optional response body; `None` sends an empty body.
    pub body: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Structured values used to construct CORS response headers.
pub struct CorsOp {
    /// Value for `Access-Control-Allow-Origin`.
    pub origin: Value,
    /// Optional space-separated allowed methods.
    pub methods: Option<Value>,
    /// Optional space-separated allowed request headers.
    pub headers: Option<Value>,
    /// Whether credentials are allowed; absence leaves the header unset.
    pub credentials: Option<bool>,
    /// Optional space-separated headers exposed to browser code.
    pub expose: Option<Value>,
    /// Optional preflight cache duration written to `Access-Control-Max-Age`.
    pub max_age: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A response caching policy compiled from `cache(...)`.
pub enum CacheOp {
    /// Disables caching by writing no-cache `Cache-Control` and `Pragma` fields.
    Off,
    /// Serializes the ordered directives into `Cache-Control`.
    Directives(Vec<CacheDirective>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// One name or name/value token in a `Cache-Control` header.
pub struct CacheDirective {
    /// Directive name, normalized and validated during parsing.
    pub name: String,
    /// Optional directive argument, for example the seconds in `max-age=60`.
    pub value: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Origin TLS policy resolved from `tls(...)`.
///
/// This policy applies after direct, SOCKS, or proxy-CONNECT routing and never
/// changes TLS used to reach an HTTPS proxy hop itself.
pub struct TlsOp {
    /// Optional PEM certificate-chain path for origin mutual TLS.
    pub client_cert: Option<String>,
    /// Optional PEM private-key path paired with `client_cert`.
    pub client_key: Option<String>,
    /// Minimum TLS protocol version accepted from the origin.
    pub min_version: Option<TlsMinVersion>,
    /// Allowed cipher suites; an empty list leaves the runtime default set intact.
    pub ciphers: Vec<TlsCipherSuite>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Minimum origin TLS version supported by the rules DSL.
pub enum TlsMinVersion {
    /// Allows TLS 1.2 or newer.
    Tls12,
    /// Requires TLS 1.3.
    Tls13,
}

impl TlsMinVersion {
    /// Returns the stable DSL spelling of this minimum version.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tls12 => "1.2",
            Self::Tls13 => "1.3",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// A rustls/aws-lc cipher suite accepted by `tls(ciphers=...)`.
pub enum TlsCipherSuite {
    /// TLS 1.3 AES-128-GCM with SHA-256.
    Tls13Aes128GcmSha256,
    /// TLS 1.3 AES-256-GCM with SHA-384.
    Tls13Aes256GcmSha384,
    /// TLS 1.3 ChaCha20-Poly1305 with SHA-256.
    Tls13Chacha20Poly1305Sha256,
    /// TLS 1.2 ECDHE-ECDSA AES-128-GCM with SHA-256.
    Tls12EcdheEcdsaAes128GcmSha256,
    /// TLS 1.2 ECDHE-ECDSA AES-256-GCM with SHA-384.
    Tls12EcdheEcdsaAes256GcmSha384,
    /// TLS 1.2 ECDHE-ECDSA ChaCha20-Poly1305 with SHA-256.
    Tls12EcdheEcdsaChacha20Poly1305Sha256,
    /// TLS 1.2 ECDHE-RSA AES-128-GCM with SHA-256.
    Tls12EcdheRsaAes128GcmSha256,
    /// TLS 1.2 ECDHE-RSA AES-256-GCM with SHA-384.
    Tls12EcdheRsaAes256GcmSha384,
    /// TLS 1.2 ECDHE-RSA ChaCha20-Poly1305 with SHA-256.
    Tls12EcdheRsaChacha20Poly1305Sha256,
}

impl TlsCipherSuite {
    /// Returns the suite's stable IANA name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tls13Aes128GcmSha256 => "TLS_AES_128_GCM_SHA256",
            Self::Tls13Aes256GcmSha384 => "TLS_AES_256_GCM_SHA384",
            Self::Tls13Chacha20Poly1305Sha256 => "TLS_CHACHA20_POLY1305_SHA256",
            Self::Tls12EcdheEcdsaAes128GcmSha256 => "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
            Self::Tls12EcdheEcdsaAes256GcmSha384 => "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
            Self::Tls12EcdheEcdsaChacha20Poly1305Sha256 => {
                "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256"
            }
            Self::Tls12EcdheRsaAes128GcmSha256 => "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
            Self::Tls12EcdheRsaAes256GcmSha384 => "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
            Self::Tls12EcdheRsaChacha20Poly1305Sha256 => {
                "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256"
            }
        }
    }

    /// Reports whether the suite is defined for TLS 1.3 rather than TLS 1.2.
    pub fn is_tls13(self) -> bool {
        matches!(
            self,
            Self::Tls13Aes128GcmSha256
                | Self::Tls13Aes256GcmSha384
                | Self::Tls13Chacha20Poly1305Sha256
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A buffered body transformation.
///
/// Regex replacement operates on UTF-8 bodies; callers preserve an unchanged
/// body when content or size constraints prevent a safe rewrite.
pub enum BodyOp {
    /// Replaces the entire body with the resolved value.
    Set(Value),
    /// Inserts the resolved value before the existing body.
    Prepend(Value),
    /// Inserts the resolved value after the existing body.
    Append(Value),
    /// Replaces every regex match in a UTF-8 body.
    Replace {
        /// Regex compiled when the rule is parsed.
        pattern: RegexReplacePattern,
        /// Rust-regex replacement text with capture expansion.
        replacement: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Response media family required for an injection to run.
pub enum InjectTarget {
    /// HTML responses.
    Html,
    /// JavaScript responses.
    Js,
    /// CSS responses.
    Css,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Placement of injected content relative to the existing response body.
pub enum InjectMode {
    /// Adds content after the existing body.
    Append,
    /// Adds content before the existing body.
    Prepend,
    /// Replaces the existing body with the injected content.
    Replace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A content-type-gated response injection.
pub struct InjectOp {
    /// Response media family that permits the injection.
    pub target: InjectTarget,
    /// Binary-preserving content loaded or rendered by the caller.
    pub value: Value,
    /// Placement relative to the existing response body.
    pub mode: InjectMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Pattern applied by [`Action::UrlRewrite`] to the request path and query.
pub enum UrlRewritePattern {
    /// Literal string replacement with a structured replacement value.
    Plain(Value),
    /// Regular-expression replacement with capture expansion.
    Regex(RegexReplacePattern),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Request/response phase used by delay and throttling actions.
pub enum Phase {
    /// Before or during upstream request forwarding.
    Req,
    /// Before or during downstream response forwarding.
    Res,
}

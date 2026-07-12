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
pub enum Action {
    Host(HostPool),
    Upstream(Value),
    Direct,
    Mock(Value),
    MockRaw(Value),
    Status(u16),
    Redirect { url: Value, code: u16 },
    ReqHeader(HeaderOp),
    ResHeader(HeaderOp),
    ResStatus(u16),
    ReqMethod(Value),
    ReqCookie(CookieOp),
    ResCookie(CookieOp),
    ReqUa(Value),
    ReqReferer(Value),
    ReqAuth(Value),
    ReqForwarded(Value),
    ReqType(Value),
    ReqCharset(Value),
    ResCors(CorsOp),
    ResType(Value),
    ResCharset(Value),
    ResMerge(Value),
    ResTrailer(HeaderOp),
    Attachment(Option<Value>),
    Cache(CacheOp),
    Tls(TlsOp),
    UrlRewrite { from: UrlRewritePattern, to: Value },
    UrlQuery(Vec<QueryOp>),
    Delete(Vec<DeleteOp>),
    ReqBody(BodyOp),
    ResBody(BodyOp),
    Inject(InjectOp),
    Delay { phase: Phase, millis: u64 },
    Throttle { phase: Phase, bytes_per_sec: u64 },
    Bypass,
    Hide,
    Tag(Value),
    Skip(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeaderOp {
    Set {
        name: String,
        value: Value,
    },
    Remove {
        name: String,
    },
    Replace {
        name: String,
        pattern: RegexReplacePattern,
        replacement: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryOp {
    Set { name: String, value: Value },
    Remove { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteOp {
    Pathname,
    PathSegment(DeletePathSegment),
    UrlParams,
    UrlParam(String),
    ReqHeader(String),
    ResHeader(String),
    ReqBody,
    ResBody,
    ReqBodyPath(DeleteBodyPath),
    ResBodyPath(DeleteBodyPath),
    ReqType,
    ResType,
    ReqCharset,
    ResCharset,
    ReqCookie(String),
    ResCookie(String),
    ReqCookies,
    ResCookies,
    Trailer(String),
    Trailers,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CookieOp {
    Set {
        name: String,
        value: Value,
        attrs: Vec<CookieAttr>,
    },
    Remove {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CookieAttr {
    pub name: String,
    pub value: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorsOp {
    pub origin: Value,
    pub methods: Option<Value>,
    pub headers: Option<Value>,
    pub credentials: Option<bool>,
    pub expose: Option<Value>,
    pub max_age: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheOp {
    Off,
    Directives(Vec<CacheDirective>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheDirective {
    pub name: String,
    pub value: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsOp {
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
    pub min_version: Option<TlsMinVersion>,
    pub ciphers: Vec<TlsCipherSuite>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsMinVersion {
    Tls12,
    Tls13,
}

impl TlsMinVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tls12 => "1.2",
            Self::Tls13 => "1.3",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsCipherSuite {
    Tls13Aes128GcmSha256,
    Tls13Aes256GcmSha384,
    Tls13Chacha20Poly1305Sha256,
    Tls12EcdheEcdsaAes128GcmSha256,
    Tls12EcdheEcdsaAes256GcmSha384,
    Tls12EcdheEcdsaChacha20Poly1305Sha256,
    Tls12EcdheRsaAes128GcmSha256,
    Tls12EcdheRsaAes256GcmSha384,
    Tls12EcdheRsaChacha20Poly1305Sha256,
}

impl TlsCipherSuite {
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
pub enum BodyOp {
    Set(Value),
    Prepend(Value),
    Append(Value),
    Replace {
        pattern: RegexReplacePattern,
        replacement: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectTarget {
    Html,
    Js,
    Css,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectMode {
    Append,
    Prepend,
    Replace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectOp {
    pub target: InjectTarget,
    pub value: Value,
    pub mode: InjectMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UrlRewritePattern {
    Plain(Value),
    Regex(RegexReplacePattern),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Req,
    Res,
}

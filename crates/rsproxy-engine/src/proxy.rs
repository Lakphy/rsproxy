use crate::state::{ProxyConfig, SharedState, UpstreamRootCache};
#[cfg(test)]
use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose,
};
use rsproxy_net::{
    self as http, BoundedBody, DownstreamH2Config, DownstreamH2Request, DownstreamH2RequestFrame,
    DownstreamH2Response, DownstreamH2ResponseFrame, DownstreamH2ResponseHead, H2Body, H2Config,
    H2Connected, H2DispatchRequest, H2Outcome, RawRequest, ReadyIo, StreamingH2Request,
    UpstreamBody, UpstreamBodyFrame, UpstreamH2Request, UpstreamH2Response,
    dispatch as dispatch_upstream_h2, serve_downstream_h2,
};
use rsproxy_net::{RequestDeadline, TimeoutBudget, is_request_total_timeout};
use rsproxy_rules::{
    Action, BodyOp, CacheOp, CookieOp, CorsOp, DeleteOp, DeletePathSegment, HeaderOp, InjectMode,
    InjectOp, InjectTarget, MatchedRule, Phase, QueryOp, RequestMeta, ResolvedAction, ResponseMeta,
    RuleSet, TlsCipherSuite, TlsMinVersion, TlsOp, UrlParts, UrlRewritePattern, Value,
};
use rsproxy_trace::{
    FrameDataEncoding, FrameDirection, FrameRecord, Session, SessionKind, TlsRecord,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{
    CipherSuite, ClientConfig, ClientConnection, ProtocolVersion, RootCertStore, ServerConfig,
    ServerConnection, StreamOwned,
};
use serde_json::Value as JsonValue;
use std::fs;
use std::io::{self, BufReader, Cursor, Read, Write};
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};

mod auth;
mod body;
mod client_tls;
mod connect;
mod connect_proxy;
mod connect_tls;
mod cookies;
mod deadline_io;
mod forward;
mod h1_forward;
mod h2_bridge;
mod http_flow;
mod mock;
mod request_actions;
mod request_stream;
mod request_util;
mod response_actions;
mod routing;
mod server;
mod stream;
pub(crate) mod tls;
mod trace_helpers;
mod transforms;
mod tunnel;
mod upstream_response;
mod websocket;
mod websocket_forward;
mod websocket_frame;

pub use server::serve;
pub(crate) use stream::UpstreamStream;
pub(crate) use tls::initialize_upstream_roots;

use auth::*;
use body::*;
use client_tls::*;
use connect::*;
use connect_proxy::*;
use connect_tls::*;
use cookies::*;
use deadline_io::*;
use forward::*;
use h2_bridge::prepare_h2_client_response_headers;
use http_flow::*;
use mock::*;
use request_actions::*;
use request_stream::*;
use request_util::*;
use response_actions::*;
use routing::*;
use server::{is_h1_request_input_error, write_h1_request_input_error};
use stream::{
    ClientPersistence, ForwardResult, NetworkTimings, UpstreamProtocol, WsIo,
    client_response_version, header_contains_token, requested_client_connection,
};
use tls::*;
use trace_helpers::*;
use transforms::*;
use tunnel::*;
use upstream_response::*;
use websocket::*;
use websocket_frame::*;

#[cfg(test)]
use h2_bridge::{CapturedHttpResponse, process_h2_request_collected};
#[cfg(test)]
use server::handle_client;

const HTTP1_ALPN: &[u8] = b"http/1.1";
const H2_ALPN: &[u8] = b"h2";
const CLIENT_KEEPALIVE_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const UPSTREAM_READ_TIMEOUT: Duration = Duration::from_secs(60);
const UPSTREAM_WRITE_TIMEOUT: Duration = Duration::from_secs(30);

struct TlsClientIdentity {
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
}

#[cfg(test)]
#[path = "proxy/tests/mod.rs"]
mod tests;

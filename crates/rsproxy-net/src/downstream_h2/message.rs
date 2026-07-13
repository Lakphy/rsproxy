use super::body::{channel_body, response_parts};
use super::{DownstreamH2Body, DownstreamH2Config, DownstreamH2Response};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{HeaderMap, Request, Response, StatusCode};
use std::convert::Infallible;

use crate::{self as http, RawRequest};

pub(super) fn validate_request_headers(
    request: &Request<Incoming>,
    config: &DownstreamH2Config,
) -> Result<(), DownstreamH2RequestError> {
    let count = request.headers().len().saturating_add(4);
    if count > config.max_header_count {
        return Err(DownstreamH2RequestError::new(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            format!(
                "header count limit exceeded (limit {})",
                config.max_header_count
            ),
        ));
    }
    let mut size = request.method().as_str().len() + request.uri().to_string().len() + 128;
    for (name, value) in request.headers() {
        size = size
            .saturating_add(name.as_str().len())
            .saturating_add(value.as_bytes().len())
            .saturating_add(32);
    }
    if size > config.max_header_size {
        return Err(DownstreamH2RequestError::new(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            format!(
                "header size limit exceeded (limit {})",
                config.max_header_size
            ),
        ));
    }
    Ok(())
}

pub(super) fn raw_request_head(
    parts: hyper::http::request::Parts,
    connect_authority: &str,
) -> Result<(RawRequest, String), DownstreamH2RequestError> {
    let authority = parts
        .uri
        .authority()
        .map(|value| value.as_str().to_string())
        .or_else(|| {
            parts
                .headers
                .get(hyper::header::HOST)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        })
        .unwrap_or_else(|| connect_authority.to_string());
    let target = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    let mut headers = Vec::with_capacity(parts.headers.len() + 1);
    for (name, value) in &parts.headers {
        if h2_forbidden_header(name.as_str()) {
            continue;
        }
        let value = value.to_str().map_err(|_| {
            DownstreamH2RequestError::new(
                StatusCode::BAD_REQUEST,
                format!("HTTP/2 header `{name}` is not valid UTF-8"),
            )
        })?;
        headers.push((name.as_str().to_string(), value.to_string()));
    }
    http::remove_header(&mut headers, "proxy-authorization");
    http::set_header(&mut headers, "Host", authority.clone());
    Ok((
        RawRequest {
            method: parts.method.as_str().to_string(),
            target,
            version: "HTTP/2".to_string(),
            headers,
            body: Vec::new(),
            trailers: Vec::new(),
        },
        authority,
    ))
}

#[cfg(test)]
pub(super) fn raw_request(
    parts: hyper::http::request::Parts,
    body: Vec<u8>,
    trailers: Vec<(String, String)>,
    connect_authority: &str,
) -> Result<(RawRequest, String), DownstreamH2RequestError> {
    let (mut request, authority) = raw_request_head(parts, connect_authority)?;
    request.body = body;
    request.trailers = trailers;
    if !request.body.is_empty() || http::header(&request.headers, "content-length").is_some() {
        http::set_header(
            &mut request.headers,
            "Content-Length",
            request.body.len().to_string(),
        );
    }
    Ok((request, authority))
}

pub(super) fn request_trailers(
    trailers: &HeaderMap,
    config: &DownstreamH2Config,
) -> Result<Vec<(String, String)>, DownstreamH2RequestError> {
    if trailers.len() > config.max_header_count {
        return Err(DownstreamH2RequestError::new(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            format!(
                "trailer count limit exceeded (limit {})",
                config.max_header_count
            ),
        ));
    }
    let mut output = Vec::with_capacity(trailers.len());
    for (name, value) in trailers {
        let value = value.to_str().map_err(|_| {
            DownstreamH2RequestError::new(
                StatusCode::BAD_REQUEST,
                format!("HTTP/2 trailer `{name}` is not valid UTF-8"),
            )
        })?;
        output.push((name.as_str().to_string(), value.to_string()));
    }
    http::validate_request_trailers(&output, config.max_header_size, config.max_header_count)
        .map_err(|error| {
            let status = if error.to_string().contains("limit exceeded") {
                StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            };
            DownstreamH2RequestError::new(status, error.to_string())
        })?;
    Ok(output)
}

pub(super) fn hyper_response(
    response: DownstreamH2Response,
) -> Result<Response<DownstreamH2Body>, DownstreamH2RequestError> {
    let DownstreamH2Response { head, body } = response;
    let (status, headers) = response_parts(head).map_err(|error| {
        DownstreamH2RequestError::new(
            StatusCode::BAD_GATEWAY,
            format!("invalid bridged response: {error}"),
        )
    })?;
    let status = StatusCode::from_u16(status).map_err(|error| {
        DownstreamH2RequestError::new(
            StatusCode::BAD_GATEWAY,
            format!("invalid bridged response status: {error}"),
        )
    })?;
    let mut response = Response::new(channel_body(body));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    Ok(response)
}

pub(super) fn error_response(status: StatusCode, message: &str) -> Response<DownstreamH2Body> {
    let body = format!("{message}\n");
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/plain")
        .header(hyper::header::CONTENT_LENGTH, body.len().to_string())
        .body(static_body(body.into_bytes()))
        .expect("static HTTP/2 error response is valid")
}

fn static_body(body: Vec<u8>) -> DownstreamH2Body {
    Full::new(Bytes::from(body))
        .map_err(|never: Infallible| match never {})
        .boxed()
}

fn h2_forbidden_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade"
    )
}

#[derive(Debug)]
pub(super) struct DownstreamH2RequestError {
    pub(super) status: StatusCode,
    pub(super) message: String,
}

impl DownstreamH2RequestError {
    pub(super) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

use super::*;

type SplitResponse = (u16, Vec<(String, String)>, hyper::body::Incoming);
use http_body_util::{BodyExt, Full};
use hyper::header::{HeaderName, HeaderValue};
use hyper::{HeaderMap, Method, Request, Uri, Version};
use std::convert::Infallible;

pub(super) fn hyper_request(
    request: UpstreamH2Request,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Request<RequestBody>> {
    crate::http::validate_request_trailers(&request.trailers, max_header_size, max_header_count)
        .map_err(|error| stage_error("request_trailer", error))?;
    let UpstreamH2Request {
        method,
        uri,
        headers,
        body,
        trailers,
    } = request;
    build_request(
        method,
        uri,
        headers,
        request_body(body, trailers)?,
        max_header_size,
        max_header_count,
    )
}

pub(super) fn hyper_request_with_body(
    request: UpstreamH2Request,
    body: RequestBody,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Request<RequestBody>> {
    crate::http::validate_request_trailers(&request.trailers, max_header_size, max_header_count)
        .map_err(|error| stage_error("request_trailer", error))?;
    build_request(
        request.method,
        request.uri,
        request.headers,
        body,
        max_header_size,
        max_header_count,
    )
}

fn build_request(
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: RequestBody,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Request<RequestBody>> {
    let method =
        Method::from_bytes(method.as_bytes()).map_err(|error| stage_error("request", error))?;
    let uri = uri
        .parse::<Uri>()
        .map_err(|error| stage_error("request", error))?;
    let pseudo_header_size = method.as_str().len()
        + uri.scheme_str().map(str::len).unwrap_or(0)
        + uri
            .authority()
            .map(|value| value.as_str().len())
            .unwrap_or(0)
        + uri
            .path_and_query()
            .map(|value| value.as_str().len())
            .unwrap_or(0)
        + 4 * 32;
    let mut output = Request::builder()
        .method(method)
        .uri(uri)
        .version(Version::HTTP_2)
        .body(body)
        .map_err(|error| stage_error("request", error))?;
    append_request_headers(output.headers_mut(), headers)?;
    validate_header_limits(
        output.headers(),
        max_header_size,
        max_header_count,
        4,
        pseudo_header_size,
        "request",
    )?;
    Ok(output)
}

fn request_body(body: Vec<u8>, trailers: Vec<(String, String)>) -> io::Result<RequestBody> {
    let trailers = if trailers.is_empty() {
        None
    } else {
        let mut output = HeaderMap::new();
        for (name, value) in trailers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| stage_error("request_trailer", error))?;
            let value = HeaderValue::from_bytes(value.as_bytes())
                .map_err(|error| stage_error("request_trailer", error))?;
            output.append(name, value);
        }
        Some(output)
    };
    Ok(Full::new(Bytes::from(body))
        .with_trailers(async move { trailers.map(Ok::<_, Infallible>) })
        .map_err(|never: Infallible| match never {})
        .boxed())
}

pub(super) fn append_request_headers(
    output: &mut HeaderMap,
    headers: Vec<(String, String)>,
) -> io::Result<()> {
    let connection_tokens = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("connection"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    for (name, value) in headers {
        if h2_forbidden_header(&name)
            || name.eq_ignore_ascii_case("host")
            || name.eq_ignore_ascii_case("proxy-authorization")
            || connection_tokens
                .iter()
                .any(|token| token.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        if name.eq_ignore_ascii_case("te") && !te_is_trailers_only(&value) {
            continue;
        }
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| stage_error("request_header", error))?;
        let value = HeaderValue::from_bytes(value.as_bytes())
            .map_err(|error| stage_error("request_header", error))?;
        output.append(name, value);
    }
    Ok(())
}

pub(super) fn split_response(
    response: hyper::Response<hyper::body::Incoming>,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<SplitResponse> {
    let (parts, body) = response.into_parts();
    validate_header_limits(
        &parts.headers,
        max_header_size,
        max_header_count,
        1,
        parts.status.as_str().len() + 32,
        "response",
    )?;
    let headers = header_vec(&parts.headers, "response")?;
    Ok((parts.status.as_u16(), headers, body))
}

pub(super) fn response_trailers(
    trailers: &HeaderMap,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Vec<(String, String)>> {
    validate_header_limits(
        trailers,
        max_header_size,
        max_header_count,
        0,
        0,
        "response trailer",
    )?;
    header_vec(trailers, "response_trailer")
}

fn header_vec(headers: &HeaderMap, stage: &str) -> io::Result<Vec<(String, String)>> {
    headers
        .iter()
        .filter(|(name, _)| !h2_forbidden_header(name.as_str()))
        .map(|(name, value)| {
            let value = value.to_str().map_err(|error| stage_error(stage, error))?;
            Ok((name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

pub(super) fn validate_header_limits(
    headers: &HeaderMap,
    max_header_size: usize,
    max_header_count: usize,
    pseudo_header_count: usize,
    pseudo_header_size: usize,
    kind: &str,
) -> io::Result<()> {
    if headers.len().saturating_add(pseudo_header_count) > max_header_count {
        return Err(stage_error(
            kind,
            format!("header count limit exceeded (limit {max_header_count})"),
        ));
    }
    let size = headers
        .iter()
        .fold(pseudo_header_size, |size, (name, value)| {
            size.saturating_add(name.as_str().len())
                .saturating_add(value.as_bytes().len())
                .saturating_add(32)
        });
    if size > max_header_size {
        return Err(stage_error(
            kind,
            format!("header size limit exceeded (limit {max_header_size})"),
        ));
    }
    Ok(())
}

fn h2_forbidden_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade"
    )
}

fn te_is_trailers_only(value: &str) -> bool {
    let mut tokens = value
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty());
    let Some(first) = tokens.next() else {
        return false;
    };
    first.eq_ignore_ascii_case("trailers")
        && tokens.all(|token| token.eq_ignore_ascii_case("trailers"))
}
